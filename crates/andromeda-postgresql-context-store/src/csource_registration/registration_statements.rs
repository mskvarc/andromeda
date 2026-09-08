//! The registration statements the read and write sides build on.

use crate::csource_registration::registration_store::persistence_error;
use andromeda_ngsi_ld_model::registration::{CsrOperation, CsrStatus, ForwardingMode};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::now,
    literal::text_literal,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::{geometry_as_geojson, validated_geom_from_ewkb},
};
use andromeda_registration::{error::RegistrationError, store::ForwardingOutcome};
use sea_query::{CaseStatement, Expr as SeaExpr, ExprTrait, Keyword, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

/// Add the full registration row projection to `select`, decoding each stored
/// geometry back to `GeoJSON` and each enum to text under the column name the
/// mapper reads it by.
pub(super) fn apply_csr_projection(select: &mut SelectStatement) {
    select
        .column(Csr::Id)
        .column(Csr::Type)
        .column(Csr::RegistrationName)
        .column(Csr::Description)
        .column(Csr::Information)
        .column(Csr::DatasetId)
        .column(Csr::Tenant)
        .column(Csr::ObservationInterval)
        .column(Csr::ManagementInterval)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::Location)), Csr::Location)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::ObservationSpace)), Csr::ObservationSpace)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::OperationSpace)), Csr::OperationSpace)
        .column(Csr::ExpiresAt)
        .column(Csr::Endpoint)
        .column(Csr::Accept)
        .expr_as(cast(SeaExpr::col(Csr::Mode), PostgresType::Text), Csr::Mode)
        .column(Csr::Operations)
        .column(Csr::Scope)
        .column(Csr::RefreshRate)
        .column(Csr::Management)
        .column(Csr::ContextSourceInfo)
        .column(Csr::JsonldContext)
        .column(Csr::ContextSourceAlias)
        .expr_as(cast(SeaExpr::col(Csr::Status), PostgresType::Text), Csr::Status)
        .column(Csr::TimesSent)
        .column(Csr::TimesFailed)
        .column(Csr::LastSuccess)
        .column(Csr::LastFailure)
        .column(Csr::CreatedAt)
        .column(Csr::ModifiedAt)
        .column(Csr::AdditionalProperties);
}

/// Build a registration selection statement with the canonical projection.
pub(super) fn csr_base_select() -> SelectStatement {
    let mut select = Query::select();
    apply_csr_projection(&mut select);
    select.from(Csr::Table);
    select
}

/// The `CASE WHEN $marker IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($marker)) ELSE NULL END`
/// a geometry column is written through: the EWKB carries its SRID, and a NULL
/// EWKB projects to a NULL geometry rather than a call on a NULL argument.
pub(super) fn geometry_write(marker: usize) -> SimpleExpr {
    CaseStatement::new()
        .case(
            SeaExpr::expr(external_placeholder(marker)).is_not_null(),
            validated_geom_from_ewkb(external_placeholder(marker)),
        )
        .finally(Keyword::Null)
        .into()
}

/// The registration insert, its values bound by the caller's `.bind` chain in
/// the marker order this emits.
pub(super) fn csr_create_sql() -> Result<String, RegistrationError> {
    let mut insert = Query::insert();
    insert.into_table(Csr::Table).columns([
        Csr::Id,
        Csr::Type,
        Csr::RegistrationName,
        Csr::Description,
        Csr::Information,
        Csr::DatasetId,
        Csr::Tenant,
        Csr::ObservationInterval,
        Csr::ManagementInterval,
        Csr::Location,
        Csr::ObservationSpace,
        Csr::OperationSpace,
        Csr::ExpiresAt,
        Csr::Endpoint,
        Csr::Accept,
        Csr::Mode,
        Csr::Operations,
        Csr::Scope,
        Csr::RefreshRate,
        Csr::Management,
        Csr::ContextSourceInfo,
        Csr::JsonldContext,
        Csr::ContextSourceAlias,
        Csr::AdditionalProperties,
        Csr::CreatedAt,
        Csr::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            text_literal("ContextSourceRegistration"),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            geometry_write(9),
            geometry_write(10),
            geometry_write(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            cast(external_placeholder(15), PostgresType::CsrMode),
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            external_placeholder(22),
            external_placeholder(23),
            now(),
            now(),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The registration replacement, bound by the caller's `.bind` chain.
pub(super) fn csr_update_sql() -> String {
    let mut update = Query::update();
    update
        .table(Csr::Table)
        .values([
            (Csr::RegistrationName, external_placeholder(2)),
            (Csr::Description, external_placeholder(3)),
            (Csr::Information, external_placeholder(4)),
            (Csr::DatasetId, external_placeholder(5)),
            (Csr::Tenant, external_placeholder(6)),
            (Csr::ObservationInterval, external_placeholder(7)),
            (Csr::ManagementInterval, external_placeholder(8)),
            (Csr::Location, geometry_write(9)),
            (Csr::ObservationSpace, geometry_write(10)),
            (Csr::OperationSpace, geometry_write(11)),
            (Csr::ExpiresAt, external_placeholder(12)),
            (Csr::Endpoint, external_placeholder(13)),
            (Csr::Accept, external_placeholder(14)),
            (Csr::Mode, cast(external_placeholder(15), PostgresType::CsrMode)),
            (Csr::Operations, external_placeholder(16)),
            (Csr::Scope, external_placeholder(17)),
            (Csr::RefreshRate, external_placeholder(18)),
            (Csr::Management, external_placeholder(19)),
            (Csr::ContextSourceInfo, external_placeholder(20)),
            (Csr::JsonldContext, external_placeholder(21)),
            (Csr::ContextSourceAlias, external_placeholder(22)),
            (Csr::AdditionalProperties, external_placeholder(23)),
            (Csr::ModifiedAt, now()),
        ])
        .and_where(SeaExpr::col(Csr::Id).eq(external_placeholder(1)));
    update.to_string(PostgresQueryBuilder)
}

/// Encode supported operations using their PostgreSQL enum labels.
pub(super) fn operation_names(operations: &[CsrOperation]) -> Vec<String> {
    operations.iter().map(ToString::to_string).collect()
}

/// Encode the `datasetId` list in its clause 4.5.5 wire form.
pub(super) fn dataset_selectors(dataset_id: Option<&[DatasetSelector]>) -> Option<Vec<String>> {
    dataset_id.map(|values| values.iter().map(ToString::to_string).collect())
}

/// The health update one forwarding outcome applies (Table 5.2.9-2): the sent
/// counter always grows; the failed counter, the matching timestamp and the
/// status follow the outcome.
pub(super) fn health_update_sql(outcome: ForwardingOutcome) -> String {
    let mut update = Query::update();
    update
        .table(Csr::Table)
        .value(Csr::TimesSent, SeaExpr::col(Csr::TimesSent).add(1))
        .and_where(SeaExpr::col(Csr::Id).eq(external_placeholder(1)));
    match outcome {
        ForwardingOutcome::Succeeded => {
            update
                .value(Csr::LastSuccess, external_placeholder(2))
                .value(Csr::Status, cast(text_literal(<&str>::from(CsrStatus::Ok)), PostgresType::CsrStatus));
        }
        ForwardingOutcome::Failed => {
            update
                .value(Csr::TimesFailed, SeaExpr::col(Csr::TimesFailed).add(1))
                .value(Csr::LastFailure, external_placeholder(2))
                .value(Csr::Status, cast(text_literal(<&str>::from(CsrStatus::Failed)), PostgresType::CsrStatus));
        }
    }
    update.to_string(PostgresQueryBuilder)
}

/// The predicate matching proxied registrations: mode `exclusive` or `redirect`.
pub(super) fn proxied_mode() -> SimpleExpr {
    cast(SeaExpr::col(Csr::Mode), PostgresType::Text).is_in([<&str>::from(ForwardingMode::Exclusive), <&str>::from(ForwardingMode::Redirect)])
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_statements::{csr_create_sql, csr_update_sql, health_update_sql};
    use andromeda_registration::store::ForwardingOutcome;

    #[test]
    fn the_create_writes_its_type_geometries_and_mode_with_contiguous_markers() {
        let sql = csr_create_sql().expect("the fixed registration insert builds");
        assert!(sql.contains("'ContextSourceRegistration'"), "{sql}");
        assert!(sql.contains("$15::csr_mode"), "{sql}");
        for marker in [9, 10, 11] {
            assert!(
                sql.contains(&format!("ST_MakeValid(ST_GeomFromEWKB(${marker}))")),
                "geometry {marker} is validated from its EWKB: {sql}"
            );
        }
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        for marker in 1..=23 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
    }

    #[test]
    fn the_update_pins_the_row_by_id_and_decodes_its_geometries() {
        let sql = csr_update_sql();
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains("$15::csr_mode"), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
        assert!(sql.contains("ST_MakeValid(ST_GeomFromEWKB($9))"), "{sql}");
    }

    #[test]
    fn a_success_stamps_last_success_and_an_ok_status_while_a_failure_counts_and_stamps_the_failure() {
        let success = health_update_sql(ForwardingOutcome::Succeeded);
        assert!(success.contains(r#""times_sent" = "times_sent" + 1"#), "{success}");
        assert!(success.contains(r#""last_success" = $2"#), "{success}");
        assert!(success.contains("'ok'::csr_status"), "{success}");
        assert!(!success.contains("times_failed"), "{success}");
        let failure = health_update_sql(ForwardingOutcome::Failed);
        assert!(failure.contains(r#""times_failed" = "times_failed" + 1"#), "{failure}");
        assert!(failure.contains(r#""last_failure" = $2"#), "{failure}");
        assert!(failure.contains("'failed'::csr_status"), "{failure}");
        assert!(failure.contains(r#""id" = ($1)"#), "{failure}");
    }
}
