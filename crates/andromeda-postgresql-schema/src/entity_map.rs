//! The relations an `EntityMap` occupies (clauses 5.2.39 and 5.5.14).
//!
//! An `entity_maps` row is the map's control record; `entity_map_entries` holds
//! one row per (entity, source) and `entity_map_links` the `EntityMap` each
//! contributing context source created. The surrogate `entity_map_key` is never
//! rendered: every statement that starts from the map's URI resolves it with a
//! scalar subquery on the unique `id`.

use sea_query::Iden;

/// The `entity_maps` table - one control record per `EntityMap`.
#[derive(Iden, Clone, Copy)]
pub enum EntityMaps {
    /// Table name: `entity_maps`
    Table,
    /// The database-issued surrogate the child tables reference.
    EntityMapKey,
    /// Creation instant.
    CreatedAt,
    /// Expiry instant, after which the map is gone.
    ExpiresAt,
    /// The map's URI identifier.
    Id,
    /// The `<operation>:<sha256 hex>` digest of the creating request's
    /// filter parameters (clause 5.5.14), absent on maps created before it
    /// was recorded.
    RequestFingerprint,
}

/// The `entity_map_entries` table - one (entity, source) pair per row.
#[derive(Iden, Clone, Copy)]
pub enum EntityMapEntries {
    /// Table name: `entity_map_entries`
    Table,
    /// Foreign key to the owning `entity_maps` row's surrogate.
    EntityMapKey,
    /// The order the query produced the pair in.
    Position,
    /// The matched entity's URI identifier.
    EntityId,
    /// `@none` or the registration id holding the entity.
    Source,
}

/// The `entity_map_links` table - one contributing context source per row.
#[derive(Iden, Clone, Copy)]
pub enum EntityMapLinks {
    /// Table name: `entity_map_links`
    Table,
    /// Foreign key to the owning `entity_maps` row's surrogate.
    EntityMapKey,
    /// The contributing registration's URI identifier.
    RegistrationId,
    /// The `EntityMap` the context source created for the same query.
    LinkedMapId,
}

#[cfg(test)]
mod tests {
    use crate::entity_map::{EntityMapEntries, EntityMapLinks, EntityMaps};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn entity_map_relation_and_column_names() {
        assert_eq!(EntityMaps::Table.to_string(), "entity_maps");
        assert_eq!(EntityMaps::EntityMapKey.to_string(), "entity_map_key");
        assert_eq!(EntityMaps::ExpiresAt.to_string(), "expires_at");
        assert_eq!(EntityMaps::RequestFingerprint.to_string(), "request_fingerprint");
        assert_eq!(EntityMapEntries::Table.to_string(), "entity_map_entries");
        assert_eq!(EntityMapEntries::EntityId.to_string(), "entity_id");
        assert_eq!(EntityMapLinks::Table.to_string(), "entity_map_links");
        assert_eq!(EntityMapLinks::LinkedMapId.to_string(), "linked_map_id");
    }
}
