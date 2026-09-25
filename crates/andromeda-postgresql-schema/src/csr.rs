//! The `context_source_registrations` relation and its columns.
//!
//! Named through an [`Iden`] enum rather than hand-quoted strings: a column
//! rename moves every reference with it, and a typo becomes a compile error
//! instead of a query that silently selects nothing. Some columns still project
//! through a cast sea-query cannot express — `ST_AsGeoJSON(...)::JSONB` and
//! `::text[]` — but even those name their column and their output alias through
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
    /// Human-readable registration name.
    RegistrationName,
    /// Free-text description.
    Description,
    /// The `information` array of registered entity/attribute coverage.
    Information,
    /// The `datasetId` list of attribute instances the source provides.
    DatasetId,
    /// Owning tenant.
    Tenant,
    /// The `observationInterval` temporal bounds.
    ObservationInterval,
    /// The `managementInterval` temporal bounds.
    ManagementInterval,
    /// `PostGIS` geometry of the registration `location`.
    Location,
    /// `PostGIS` geometry of the `observationSpace`.
    ObservationSpace,
    /// `PostGIS` geometry of the `operationSpace`.
    OperationSpace,
    /// Expiry instant, after which the registration is gone.
    ExpiresAt,
    /// The context source endpoint URI.
    Endpoint,
    /// The endpoint's `accept` media type.
    Accept,
    /// The forwarding mode code (`CsrModeCode`).
    Mode,
    /// The operation codes the source serves (`CsrOperationCode`).
    Operations,
    /// The registration's scopes, as `ltree` label paths.
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
    /// Creation instant (clause 4.8 system attribute).
    CreatedAt,
    /// Last-modification instant (clause 4.8 system attribute).
    ModifiedAt,
    /// The `additionalProperties` JSONB bag of unmodelled members.
    AdditionalProperties,
}

/// The `csr_delivery_state` relation: the forwarding statistics and health of
/// one registration (Table 5.2.9-2), split off the registration row because
/// every forwarded request moves them.
#[derive(Iden, Clone, Copy)]
pub enum CsrDeliveryState {
    /// The `csr_delivery_state` table.
    Table,
    /// Primary key and foreign key to the owning registration.
    RegistrationId,
    /// Count of forwarded requests.
    TimesSent,
    /// Count of forwarded requests that failed.
    TimesFailed,
    /// Instant of the last successful contact.
    LastSuccess,
    /// Instant of the last failed contact.
    LastFailure,
    /// The health status code (`CsrStatusCode`).
    Status,
}

/// The `registration_information` routing projection derived from a
/// registration's `information`, which distributed-operation discovery
/// preselects against before reading the payload.
///
/// Every axis is `NOT NULL` and carries a sentinel where the row does not
/// constrain it: `{'*'}` for the type set, `''` for the identifier, `{0}` for
/// both attribute-key arrays. A registration constraining nothing is one row
/// carrying every sentinel.
#[derive(Iden, Clone, Copy)]
pub enum RegistrationInformation {
    /// The `registration_information` table.
    Table,
    /// Foreign key to the owning `context_source_registrations` row.
    RegistrationId,
    /// The set of entity-type IRIs the information item constrains, or `{'*'}`
    /// when it constrains no type.
    EntityTypes,
    /// The exact entity identifier the item constrains, or `''` when it names
    /// none.
    EntityId,
    /// The entity identifier pattern the item constrains, or `NULL`.
    EntityIdPattern,
    /// The interned property-name dictionary keys the item declares; `{0}` when
    /// it declares no name on either axis, `{}` when it declares names on the
    /// relationship axis only.
    PropertyNameIds,
    /// The interned relationship-name dictionary keys the item declares; `{0}`
    /// when it declares no name on either axis, `{}` when it declares names on
    /// the property axis only.
    RelationshipNameIds,
    /// The registration-wide clause 4.20 operation codes, replicated onto every
    /// row, or `NULL` when the registration declares none.
    Operations,
}

#[cfg(test)]
mod tests {
    use crate::csr::{Csr, CsrDeliveryState, RegistrationInformation};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn csr_table_name() {
        assert_eq!(Csr::Table.to_string(), "context_source_registrations");
    }

    #[test]
    fn csr_column_names() {
        assert_eq!(Csr::Id.to_string(), "id");
        assert_eq!(Csr::RegistrationName.to_string(), "registration_name");
        assert_eq!(Csr::DatasetId.to_string(), "dataset_id");
        assert_eq!(Csr::ObservationInterval.to_string(), "observation_interval");
        assert_eq!(Csr::ObservationSpace.to_string(), "observation_space");
        assert_eq!(Csr::JsonldContext.to_string(), "jsonld_context");
        assert_eq!(Csr::AdditionalProperties.to_string(), "additional_properties");
    }

    #[test]
    fn csr_delivery_state_column_names() {
        assert_eq!(CsrDeliveryState::Table.to_string(), "csr_delivery_state");
        assert_eq!(CsrDeliveryState::RegistrationId.to_string(), "registration_id");
        assert_eq!(CsrDeliveryState::TimesSent.to_string(), "times_sent");
        assert_eq!(CsrDeliveryState::Status.to_string(), "status");
    }

    #[test]
    fn registration_information_column_names() {
        assert_eq!(RegistrationInformation::Table.to_string(), "registration_information");
        assert_eq!(RegistrationInformation::EntityTypes.to_string(), "entity_types");
        assert_eq!(RegistrationInformation::PropertyNameIds.to_string(), "property_name_ids");
        assert_eq!(RegistrationInformation::Operations.to_string(), "operations");
    }
}
