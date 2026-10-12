//! The one projection every registration read shares.
//!
//! Point reads, listings, the expiry sweep, the overlap check and discovery
//! (clause 5.10.2) all decode the same row, so they select it through one
//! statement: the registration's own columns joined to its delivery state.
//! Geometries are decoded to `GeoJSON` and the `ltree[]` scopes to their text
//! label paths under the column names the store's mapper reads them by; the
//! mode and operations stay the registry codes they are stored as. The
//! registration's `information` is not on this row: it is assembled from its
//! `registration_information` rows, which a second statement reads.
//!
//! The delivery state is `LEFT JOIN`ed on the registration's primary key. A
//! registration always has its row -- the create transaction writes both --
//! but a read must never lose a registration over its health, so the join does
//! not require one. No column name repeats between the two relations, so the
//! predicates callers add on registration columns stay unambiguous.

use andromeda_postgresql_schema::csr::{Csr, RegistrationDeliveryState};
use andromeda_postgresql_seaquery::postgis::geometry_as_geojson;
use sea_query::{Expr as SeaExpr, ExprTrait, JoinType, Query, SelectStatement};

/// A registration select with the canonical projection, `FROM` the
/// registrations joined to their delivery state, ready for a caller's `WHERE`.
#[must_use]
pub fn registration_select() -> SelectStatement {
    Query::select()
        .column((Csr::Table, Csr::Id))
        .column((Csr::Table, Csr::RegistrationName))
        .column((Csr::Table, Csr::Description))
        .column((Csr::Table, Csr::DatasetId))
        .column((Csr::Table, Csr::Tenant))
        .column((Csr::Table, Csr::ObservationIntervalStart))
        .column((Csr::Table, Csr::ObservationIntervalEnd))
        .column((Csr::Table, Csr::ManagementIntervalStart))
        .column((Csr::Table, Csr::ManagementIntervalEnd))
        .expr_as(geometry_as_geojson(SeaExpr::col((Csr::Table, Csr::Location))), Csr::Location)
        .expr_as(geometry_as_geojson(SeaExpr::col((Csr::Table, Csr::ObservationSpace))), Csr::ObservationSpace)
        .expr_as(geometry_as_geojson(SeaExpr::col((Csr::Table, Csr::OperationSpace))), Csr::OperationSpace)
        .column((Csr::Table, Csr::ExpiresAt))
        .column((Csr::Table, Csr::Endpoint))
        .column((Csr::Table, Csr::Accept))
        .column((Csr::Table, Csr::Mode))
        .column((Csr::Table, Csr::Operations))
        // The Scopes as written: the `scope_paths` label paths beside them are only
        // matched, since they forget the optional leading `/` of clause 4.18.
        .column((Csr::Table, Csr::VerbatimScopePaths))
        .column((Csr::Table, Csr::RefreshRate))
        .column((Csr::Table, Csr::IsManagementDeclared))
        .column((Csr::Table, Csr::ManagementCacheDuration))
        .column((Csr::Table, Csr::ManagementCooldown))
        .column((Csr::Table, Csr::ManagementLocalOnly))
        .column((Csr::Table, Csr::ManagementTimeout))
        .column((Csr::Table, Csr::ContextSourceInfoKeys))
        .column((Csr::Table, Csr::ContextSourceInfoValues))
        .column((Csr::Table, Csr::JsonldContext))
        .column((Csr::Table, Csr::ContextSourceAlias))
        .column((Csr::Table, Csr::CreatedAt))
        .column((Csr::Table, Csr::ModifiedAt))
        .column((Csr::Table, Csr::AdditionalProperties))
        .column((RegistrationDeliveryState::Table, RegistrationDeliveryState::Status))
        .column((RegistrationDeliveryState::Table, RegistrationDeliveryState::TimesSent))
        .column((RegistrationDeliveryState::Table, RegistrationDeliveryState::TimesFailed))
        .column((RegistrationDeliveryState::Table, RegistrationDeliveryState::LastSuccess))
        .column((RegistrationDeliveryState::Table, RegistrationDeliveryState::LastFailure))
        .from(Csr::Table)
        .join(
            JoinType::LeftJoin,
            RegistrationDeliveryState::Table,
            SeaExpr::col((RegistrationDeliveryState::Table, RegistrationDeliveryState::RegistrationId)).equals((Csr::Table, Csr::Id)),
        )
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::csr::registration_select::registration_select;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_projection_left_joins_the_delivery_state_on_the_registration_key() {
        let sql = registration_select().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(
                "FROM \"context_source_registrations\" LEFT JOIN \"registration_delivery_state\" ON \"registration_delivery_state\".\"registration_id\" = \"context_source_registrations\".\"id\""
            ),
            "{sql}"
        );
        for health in ["status", "times_sent", "times_failed", "last_success", "last_failure"] {
            assert!(
                sql.contains(&format!("\"registration_delivery_state\".\"{health}\"")),
                "{health} is read from the delivery state: {sql}"
            );
        }
    }

    #[test]
    fn the_projection_reads_codes_and_written_scopes_and_no_type_column() {
        let sql = registration_select().to_string(PostgresQueryBuilder);
        assert!(!sql.contains("\"type\""), "no registration type column remains: {sql}");
        assert!(
            sql.contains("\"context_source_registrations\".\"mode\","),
            "the mode is read as its code: {sql}"
        );
        assert!(sql.contains("\"context_source_registrations\".\"verbatim_scope_paths\""), "{sql}");
        assert!(!sql.contains("\"scope_paths\"::text[]"), "a Scope is never rebuilt from its label path: {sql}");
        assert!(!sql.contains("::csr_"), "no enum cast remains: {sql}");
    }

    /// The interval, `management` and `contextSourceInfo` members are read
    /// from their columns, and `information` from its rows, not from this one.
    #[test]
    fn the_projection_reads_the_member_columns_and_no_information_document() {
        let sql = registration_select().to_string(PostgresQueryBuilder);
        for column in [
            "observation_interval_start",
            "observation_interval_end",
            "management_interval_start",
            "management_interval_end",
            "is_management_declared",
            "management_cache_duration",
            "management_cooldown",
            "management_local_only",
            "management_timeout",
            "context_source_info_keys",
            "context_source_info_values",
        ] {
            assert!(sql.contains(&format!("\"context_source_registrations\".\"{column}\"")), "{column}: {sql}");
        }
        assert!(!sql.contains("\"information\""), "{sql}");
    }
}
