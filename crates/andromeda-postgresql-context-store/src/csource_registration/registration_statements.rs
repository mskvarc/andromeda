//! The registration statements the read and write sides build on.
//!
//! Every read selects through the one projection
//! [`registration_select`](andromeda_postgresql_context_query::csr::registration_select::registration_select)
//! discovery shares; this module owns the writes.

use crate::csource_registration::{
    mode_code::stored_mode,
    operation_code::stored_operation,
    registration_store::persistence_error,
    status_code::stored_status,
};
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_model::registration::{forwarding_mode::ForwardingMode, operation::CsrOperation, status::CsrStatus};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_postgresql_query::ltree::StoredScopePath;
use andromeda_postgresql_schema::csr::{Csr, RegistrationDeliveryState};
use andromeda_postgresql_seaquery::{cast::cast, clock::now, pg_type::PostgresType, placeholder::external_placeholder, postgis::validated_geom_from_ewkb};
use andromeda_registration::{error::RegistrationError, store::forwarding_outcome::ForwardingOutcome};
use sea_query::{CaseStatement, Expr as SeaExpr, ExprTrait, Keyword, PostgresQueryBuilder, Query, SimpleExpr};

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
        Csr::ScopePaths,
        Csr::RefreshRate,
        Csr::Management,
        Csr::ContextSourceInfo,
        Csr::JsonldContext,
        Csr::ContextSourceAlias,
        Csr::AdditionalProperties,
        Csr::EntityIdPatternForms,
        Csr::EntityIdPatternFormsRevision,
        Csr::VerbatimScopePaths,
        Csr::CreatedAt,
        Csr::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
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
            external_placeholder(15),
            external_placeholder(16),
            cast(external_placeholder(17), PostgresType::LtreeArray),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            external_placeholder(22),
            external_placeholder(23),
            external_placeholder(24),
            external_placeholder(25),
            external_placeholder(26),
            now(),
            now(),
        ])
        .map_err(|error| persistence_error(error))?;
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
            (Csr::Mode, external_placeholder(15)),
            (Csr::Operations, external_placeholder(16)),
            (Csr::ScopePaths, cast(external_placeholder(17), PostgresType::LtreeArray)),
            (Csr::RefreshRate, external_placeholder(18)),
            (Csr::Management, external_placeholder(19)),
            (Csr::ContextSourceInfo, external_placeholder(20)),
            (Csr::JsonldContext, external_placeholder(21)),
            (Csr::ContextSourceAlias, external_placeholder(22)),
            (Csr::AdditionalProperties, external_placeholder(23)),
            (Csr::EntityIdPatternForms, external_placeholder(24)),
            (Csr::EntityIdPatternFormsRevision, external_placeholder(25)),
            (Csr::VerbatimScopePaths, external_placeholder(26)),
            (Csr::ModifiedAt, now()),
        ])
        .and_where(SeaExpr::col(Csr::Id).eq(external_placeholder(1)));
    update.to_string(PostgresQueryBuilder)
}

/// The delivery-state row a new registration starts with: nothing sent,
/// nothing failed, and an `ok` status the first forwarding outcome replaces.
///
/// # Errors
///
/// Returns [`RegistrationError`] when the fixed insert cannot be assembled.
pub(super) fn delivery_state_insert_sql() -> Result<String, RegistrationError> {
    let mut insert = Query::insert();
    insert
        .into_table(RegistrationDeliveryState::Table)
        .columns([RegistrationDeliveryState::RegistrationId, RegistrationDeliveryState::Status])
        .values([external_placeholder(1), SeaExpr::val(stored_status(CsrStatus::Ok).code())])
        .map_err(|error| persistence_error(error))?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Encode supported operations as their stored registry codes.
///
/// An empty list is stored as the absent member rather than as an empty array:
/// the column's check admits no empty set, the same rule the routing
/// projection's replicated copy follows.
pub(super) fn operation_codes(operations: &[CsrOperation]) -> Option<Vec<i16>> {
    (!operations.is_empty()).then(|| operations.iter().map(|operation| stored_operation(*operation).code()).collect())
}

/// Encode the stored forwarding mode.
pub(super) const fn mode_code(mode: ForwardingMode) -> i16 {
    stored_mode(mode).code()
}

/// A registration's clause 4.18 scopes in the two columns they are stored in:
/// `scope_paths`, the label paths a `scopeQ` matches, and `verbatim_scope_paths`, the text
/// read back. Both are absent together, and both hold the scopes in order.
pub(super) struct StoredScopes<'a> {
    /// The `ltree` label paths, bound to `scope_paths`.
    pub(super) label_paths: Option<Vec<String>>,
    /// The scopes as written, bound to `verbatim_scope_paths`.
    pub(super) written: Option<Vec<&'a str>>,
}

/// Encode a registration's scopes through the one stored form of a Scope.
pub(super) fn stored_scopes(scopes: Option<&[ScopePath]>) -> StoredScopes<'_> {
    let (label_paths, written) = scopes
        .map(|values| {
            values
                .iter()
                .map(|scope| {
                    let stored = StoredScopePath::from(scope);
                    (stored.label_path, stored.written)
                })
                .unzip()
        })
        .unzip();
    StoredScopes { label_paths, written }
}

/// Encode the `datasetId` list in its clause 4.5.5 wire form.
pub(super) fn dataset_selectors(dataset_id: Option<&[DatasetSelector]>) -> Option<Vec<String>> {
    dataset_id.map(|values| values.iter().map(ToString::to_string).collect())
}

/// The health update one forwarding outcome applies (Table 5.2.9-2) to the
/// registration's delivery state: the sent counter always grows; the failed
/// counter, the matching timestamp and the status follow the outcome.
pub(super) fn health_update_sql(outcome: ForwardingOutcome) -> String {
    let mut update = Query::update();
    update
        .table(RegistrationDeliveryState::Table)
        .value(RegistrationDeliveryState::TimesSent, SeaExpr::col(RegistrationDeliveryState::TimesSent).add(1))
        .and_where(SeaExpr::col(RegistrationDeliveryState::RegistrationId).eq(external_placeholder(1)));
    match outcome {
        ForwardingOutcome::Succeeded => {
            update
                .value(RegistrationDeliveryState::LastSuccess, external_placeholder(2))
                .value(RegistrationDeliveryState::Status, stored_status(CsrStatus::Ok).code());
        }
        ForwardingOutcome::Failed => {
            update
                .value(
                    RegistrationDeliveryState::TimesFailed,
                    SeaExpr::col(RegistrationDeliveryState::TimesFailed).add(1),
                )
                .value(RegistrationDeliveryState::LastFailure, external_placeholder(2))
                .value(RegistrationDeliveryState::Status, stored_status(CsrStatus::Failed).code());
        }
    }
    update.to_string(PostgresQueryBuilder)
}

/// The predicate matching proxied registrations: mode `exclusive` or `redirect`.
pub(super) fn proxied_mode() -> SimpleExpr {
    SeaExpr::col((Csr::Table, Csr::Mode)).is_in([mode_code(ForwardingMode::Exclusive), mode_code(ForwardingMode::Redirect)])
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_statements::{
        csr_create_sql,
        csr_update_sql,
        delivery_state_insert_sql,
        health_update_sql,
        proxied_mode,
        stored_scopes,
    };
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use andromeda_registration::store::forwarding_outcome::ForwardingOutcome;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    /// Clause 4.18 scopes are stored as `ltree` label paths, the encoding
    /// `entity_search.scope_paths` uses, so the clause 4.19 `scopeQ` compiles to
    /// one `lquery` predicate for both relations; the text beside them is what
    /// reads back, so each Scope keeps the spelling it was written in.
    #[test]
    fn registration_scopes_bind_as_label_paths_and_written_text_in_declared_order() {
        let scopes: Vec<ScopePath> = ["/Madrid/Gardens", "Madrid", "/a/b/c"]
            .into_iter()
            .map(|raw| raw.parse::<ScopePath>().expect("valid scope path"))
            .collect();
        let stored = stored_scopes(Some(&scopes));
        assert_eq!(
            stored.label_paths,
            Some(vec!["Madrid.Gardens".to_owned(), "Madrid".to_owned(), "a.b.c".to_owned()])
        );
        assert_eq!(stored.written, Some(vec!["/Madrid/Gardens", "Madrid", "/a/b/c"]));
        let absent = stored_scopes(None);
        assert_eq!((absent.label_paths, absent.written), (None, None), "an absent scope stays NULL in both columns");
    }

    #[test]
    fn the_create_writes_no_type_and_binds_the_mode_code_and_ltree_scopes() {
        let sql = csr_create_sql().expect("the fixed registration insert builds");
        assert!(!sql.contains("ContextSourceRegistration"), "the type is never stored: {sql}");
        assert!(!sql.contains("\"type\""), "{sql}");
        assert!(!sql.contains("::csr_mode"), "{sql}");
        assert!(sql.contains("$17::ltree[]"), "{sql}");
        for marker in [9, 10, 11] {
            assert!(
                sql.contains(&format!("ST_MakeValid(ST_GeomFromEWKB(${marker}))")),
                "geometry {marker} is validated from its EWKB: {sql}"
            );
        }
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        assert!(sql.contains(r#""verbatim_scope_paths""#), "{sql}");
        for marker in 1..=26 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
    }

    /// The restatement of the registration's identifier patterns is written in
    /// the statement that writes the `information` it is derived from, on
    /// create and on replacement alike.
    #[test]
    fn the_identifier_pattern_forms_are_written_with_the_information_they_restate() {
        let create = csr_create_sql().expect("the fixed registration insert builds");
        assert!(create.contains(r#""entity_id_pattern_forms""#), "{create}");
        let update = csr_update_sql();
        assert!(update.contains(r#""entity_id_pattern_forms" = $24"#), "{update}");
        assert!(update.contains(r#""entity_id_pattern_forms_revision" = $25"#), "{update}");
    }

    #[test]
    fn the_update_pins_the_row_by_id_and_binds_the_mode_code_and_ltree_scopes() {
        let sql = csr_update_sql();
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""mode" = $15"#), "{sql}");
        assert!(sql.contains(r#""scope_paths" = $17::ltree[]"#), "{sql}");
        assert!(sql.contains(r#""verbatim_scope_paths" = $26"#), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
        assert!(sql.contains("ST_MakeValid(ST_GeomFromEWKB($9))"), "{sql}");
    }

    #[test]
    fn a_new_registration_starts_its_delivery_state_as_ok() {
        let sql = delivery_state_insert_sql().expect("the fixed delivery-state insert builds");
        assert_eq!(sql, r#"INSERT INTO "registration_delivery_state" ("registration_id", "status") VALUES ($1, 1)"#);
    }

    #[test]
    fn a_forwarding_outcome_updates_the_delivery_state_not_the_registration() {
        let success = health_update_sql(ForwardingOutcome::Succeeded);
        assert!(success.starts_with(r#"UPDATE "registration_delivery_state""#), "{success}");
        assert!(success.contains(r#""times_sent" = "times_sent" + 1"#), "{success}");
        assert!(success.contains(r#""last_success" = $2"#), "{success}");
        assert!(success.contains(r#""status" = 1"#), "{success}");
        assert!(!success.contains("times_failed"), "{success}");
        let failure = health_update_sql(ForwardingOutcome::Failed);
        assert!(failure.contains(r#""times_failed" = "times_failed" + 1"#), "{failure}");
        assert!(failure.contains(r#""last_failure" = $2"#), "{failure}");
        assert!(failure.contains(r#""status" = 2"#), "{failure}");
        assert!(failure.contains(r#""registration_id" = ($1)"#), "{failure}");
    }

    #[test]
    fn operations_are_stored_as_codes_and_an_empty_list_as_the_absent_member() {
        use crate::csource_registration::registration_statements::operation_codes;
        use andromeda_ngsi_ld_model::registration::operation::CsrOperation;
        assert_eq!(operation_codes(&[CsrOperation::CreateEntity, CsrOperation::RedirectionOps]), Some(vec![1, 48]));
        assert_eq!(operation_codes(&[]), None);
    }

    #[test]
    fn a_proxied_registration_is_exclusive_or_redirect_by_code() {
        let sql = Query::select().expr(proxied_mode()).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "context_source_registrations"."mode" IN (2, 3)"#);
    }
}
