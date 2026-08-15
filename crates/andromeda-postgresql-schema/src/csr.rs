//! The `context_source_registrations` relation and its columns.
//!
//! Named through an [`Iden`] enum rather than hand-quoted strings: a column
//! rename moves every reference with it, and a typo becomes a compile error
//! instead of a query that silently selects nothing. Some columns still project
//! through a cast sea-query cannot express — `ST_AsGeoJSON(...)::JSONB` and
//! `::text` — but even those name their column and their output alias through
//! this enum in the statement builder.

use sea_query::Iden;

/// The columns of the `context_source_registrations` relation.
#[derive(Iden, Clone, Copy)]
pub enum Csr {
    /// The `context_source_registrations` table.
    #[iden = "context_source_registrations"]
    Table,
    /// Primary key: the registration's URI identifier.
    Id,
    /// The registration type discriminator.
    Type,
    /// Human-readable registration name.
    RegistrationName,
    /// Free-text description.
    Description,
    /// The `information` array of registered entity/attribute coverage.
    Information,
    /// Owning tenant.
    Tenant,
    /// The `observationInterval` temporal bounds.
    ObservationInterval,
    /// The `managementInterval` temporal bounds.
    ManagementInterval,
    /// PostGIS geometry of the registration `location`.
    Location,
    /// PostGIS geometry of the `observationSpace`.
    ObservationSpace,
    /// PostGIS geometry of the `operationSpace`.
    OperationSpace,
    /// Expiry instant, after which the registration is gone.
    ExpiresAt,
    /// The context source endpoint URI.
    Endpoint,
    /// The endpoint's `accept` media type.
    Accept,
    /// The forwarding mode enum.
    Mode,
    /// The operations the source serves.
    Operations,
    /// The registration's scope paths.
    Scope,
    /// The refresh rate the source advertises.
    RefreshRate,
    /// Management endpoint metadata.
    Management,
    /// The `contextSourceInfo` key-value pairs.
    ContextSourceInfo,
    /// The registration's JSON-LD context.
    JsonldContext,
    /// The context source alias.
    ContextSourceAlias,
    /// The forwarding status enum.
    Status,
    /// Count of notifications sent.
    TimesSent,
    /// Count of notification failures.
    TimesFailed,
    /// Instant of the last successful contact.
    LastSuccess,
    /// Instant of the last failed contact.
    LastFailure,
    /// Creation instant (clause 4.8 system attribute).
    CreatedAt,
    /// Last-modification instant (clause 4.8 system attribute).
    ModifiedAt,
    /// The `additionalProperties` JSONB bag of unmodelled members.
    AdditionalProperties,
}

/// The `registration_information` routing projection derived from a
/// registration's `information`, which distributed-operation discovery
/// preselects against before reading the payload.
#[derive(Iden, Clone, Copy)]
pub enum RegistrationInformation {
    /// The `registration_information` table.
    Table,
    /// Foreign key to the owning `context_source_registrations` row.
    RegistrationId,
    /// The set of entity-type IRIs the information item constrains, or `NULL`
    /// when it constrains no type.
    EntityTypes,
    /// The exact entity identifier the item constrains, or `NULL`.
    EntityId,
    /// The entity identifier pattern the item constrains, or `NULL`.
    EntityIdPattern,
    /// The interned property-name dictionary keys the item declares, or `NULL`.
    PropertyNameIds,
    /// The interned relationship-name dictionary keys the item declares, or `NULL`.
    RelationshipNameIds,
}

#[cfg(test)]
mod tests {
    use crate::csr::Csr;
    use sea_query::Iden;

    #[test]
    fn csr_table_name() {
        assert_eq!(Csr::Table.to_string(), "context_source_registrations");
    }

    #[test]
    fn csr_column_names() {
        assert_eq!(Csr::Id.to_string(), "id");
        assert_eq!(Csr::Type.to_string(), "type");
        assert_eq!(Csr::RegistrationName.to_string(), "registration_name");
        assert_eq!(Csr::ObservationInterval.to_string(), "observation_interval");
        assert_eq!(Csr::ObservationSpace.to_string(), "observation_space");
        assert_eq!(Csr::JsonldContext.to_string(), "jsonld_context");
        assert_eq!(Csr::AdditionalProperties.to_string(), "additional_properties");
    }
}
