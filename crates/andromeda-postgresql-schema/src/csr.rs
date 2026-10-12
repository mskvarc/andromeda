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
    /// The rendering revision that produced the registration's
    /// `registration_information.entity_id_pattern_form` values; `NULL` when it
    /// declares no `idPattern`.
    EntityIdPatternFormsRevision,
    /// The `datasetId` list of attribute instances the source provides.
    DatasetId,
    /// Owning tenant.
    Tenant,
    /// `observationInterval.startAt`; `NULL` when the member is absent.
    ObservationIntervalStart,
    /// `observationInterval.endAt`; `NULL` when absent or open.
    ObservationIntervalEnd,
    /// `managementInterval.startAt`; `NULL` when the member is absent.
    ManagementIntervalStart,
    /// `managementInterval.endAt`; `NULL` when absent or open.
    ManagementIntervalEnd,
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
    /// The registration's scopes, as `ltree` label paths a `scopeQ` matches.
    ScopePaths,
    /// The registration's scopes as they were written, in the order of `scope_paths`.
    VerbatimScopePaths,
    /// The refresh rate the source advertises.
    RefreshRate,
    /// Whether the registration declares a `management` member, empty or not.
    IsManagementDeclared,
    /// `management.cacheDuration`, the ISO 8601 duration text.
    ManagementCacheDuration,
    /// `management.cooldown`, in milliseconds.
    ManagementCooldown,
    /// `management.localOnly`.
    ManagementLocalOnly,
    /// `management.timeout`, in milliseconds.
    ManagementTimeout,
    /// The `contextSourceInfo` keys, in written order.
    ContextSourceInfoKeys,
    /// The `contextSourceInfo` values, parallel to the keys.
    ContextSourceInfoValues,
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

/// The `registration_delivery_state` relation: the forwarding statistics and health of
/// one registration (Table 5.2.9-2), split off the registration row because
/// every forwarded request moves them.
#[derive(Iden, Clone, Copy)]
pub enum RegistrationDeliveryState {
    /// The `registration_delivery_state` table.
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

/// The `registration_information` relation: a registration's `information`
/// (clause 5.2.10), one row per `EntityInfo` of an item, or one row without
/// entity columns for an item without `entities`. Absence is `NULL`.
#[derive(Iden, Clone, Copy)]
pub enum RegistrationInformation {
    /// The `registration_information` table.
    Table,
    /// Foreign key to the owning `context_source_registrations` row.
    RegistrationId,
    /// The item's position in `information`, from 0.
    ItemOrdinal,
    /// The `EntityInfo`'s position in the item's `entities`, from 0; 0 for an
    /// item without `entities`.
    SelectorOrdinal,
    /// The `EntityInfo` type names in written order; `NULL` for an item
    /// without `entities`.
    EntityTypes,
    /// How Table 5.2.8-1 `type` was written: 1 one name, 2 an array.
    EntityTypeShape,
    /// The `EntityInfo` `id`, or `NULL`.
    EntityId,
    /// The `EntityInfo` `idPattern` as written, or `NULL`.
    EntityIdPattern,
    /// The portable restatement of `EntityIdPattern` the store evaluates in
    /// its place; `NULL` exactly when the pattern is.
    EntityIdPatternForm,
    /// The item's `propertyNames` as `attribute_names` keys; `NULL` when
    /// absent.
    PropertyNameKeys,
    /// The item's `relationshipNames` as `attribute_names` keys; `NULL` when
    /// absent.
    RelationshipNameKeys,
}

#[cfg(test)]
mod tests {
    use crate::csr::{Csr, RegistrationDeliveryState, RegistrationInformation};
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
        assert_eq!(Csr::ObservationIntervalStart.to_string(), "observation_interval_start");
        assert_eq!(Csr::IsManagementDeclared.to_string(), "is_management_declared");
        assert_eq!(Csr::ContextSourceInfoKeys.to_string(), "context_source_info_keys");
        assert_eq!(Csr::ObservationSpace.to_string(), "observation_space");
        assert_eq!(Csr::JsonldContext.to_string(), "jsonld_context");
        assert_eq!(Csr::AdditionalProperties.to_string(), "additional_properties");
    }

    #[test]
    fn registration_delivery_state_column_names() {
        assert_eq!(RegistrationDeliveryState::Table.to_string(), "registration_delivery_state");
        assert_eq!(RegistrationDeliveryState::RegistrationId.to_string(), "registration_id");
        assert_eq!(RegistrationDeliveryState::TimesSent.to_string(), "times_sent");
        assert_eq!(RegistrationDeliveryState::Status.to_string(), "status");
    }

    #[test]
    fn registration_information_column_names() {
        assert_eq!(RegistrationInformation::Table.to_string(), "registration_information");
        assert_eq!(RegistrationInformation::EntityTypes.to_string(), "entity_types");
        assert_eq!(RegistrationInformation::PropertyNameKeys.to_string(), "property_name_keys");
        assert_eq!(RegistrationInformation::EntityIdPatternForm.to_string(), "entity_id_pattern_form");
        assert_eq!(RegistrationInformation::SelectorOrdinal.to_string(), "selector_ordinal");
    }
}
