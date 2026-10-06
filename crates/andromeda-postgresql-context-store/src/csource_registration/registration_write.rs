//! The write side of the registration persistence port: every mutation
//! rewrites the routing projection in the same transaction as the payload.

use crate::{
    context_source_change_clock::advance,
    csource_registration::{
        entity_id_pattern_forms::stored_pattern_forms,
        mapper::row_to_csr,
        registration_read::expired_select,
        registration_statements::{
            csr_create_sql,
            csr_update_sql,
            dataset_selectors,
            delivery_state_insert_sql,
            health_update_sql,
            mode_code,
            operation_codes,
            stored_scopes,
        },
        registration_store::{PostgresRegistrationStore, create_error, persistence_error},
        routing_rewrite::replace_registration_information,
    },
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    identifier::Identifier,
    non_empty_text::NonEmptyText,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{create_params::CreateCsrParams, expired_deletion::ExpiredDeletion, forwarding_outcome::ForwardingOutcome, mutation::RegistrationMutation},
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use sea_query::{Expr as SeaExpr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::types::Json;

/// The `additional_properties` column value: the Context Source Properties of
/// a registration, or `NULL` for a registration that has none.
fn stored_properties(properties: &IndexMap<AttributeName, JsonLiteral>) -> Option<Json<&IndexMap<AttributeName, JsonLiteral>>> {
    (!properties.is_empty()).then_some(Json(properties))
}

/// Delete every registration due at or before `cutoff`, returning the deleted rows and the revision the sweep drew; their projection rows go in the same transaction.
pub(super) async fn delete_expired(store: &PostgresRegistrationStore, cutoff: DateTime<Utc>) -> Result<ExpiredDeletion, RegistrationError> {
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error(error))?;
    // The rows to return are read before either deletion, so the projection
    // sees the geometries and enums the payload carried; the two deletions
    // then remove exactly the same set, agreeing without depending on the
    // foreign key's referential action. All three run in one transaction, so
    // no reader observes a partial removal.
    let (select_sql, select_values) = expired_select(cutoff).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(select_sql.as_str()), select_values)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    let expired = rows
        .iter()
        .map(|row| row_to_csr(row).map_err(|error| persistence_error(error)))
        .collect::<Result<Vec<_>, _>>()?;

    let mut expired_ids = Query::select();
    expired_ids
        .column(Csr::Id)
        .from(Csr::Table)
        .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
        .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff));
    let delete_information = Query::delete()
        .from_table(RegistrationInformation::Table)
        .and_where(SeaExpr::col(RegistrationInformation::RegistrationId).in_subquery(expired_ids))
        .to_owned();
    let (info_sql, info_values) = delete_information.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(info_sql.as_str()), info_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;

    let delete_registrations = Query::delete()
        .from_table(Csr::Table)
        .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
        .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff))
        .to_owned();
    let (delete_sql, delete_values) = delete_registrations.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;

    let revision = advance(&mut *transaction).await.map_err(|error| persistence_error(error))?;
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(ExpiredDeletion {
        revision,
        registrations: expired,
    })
}

/// Persist a new registration, its delivery state and its routing projection
/// in one transaction, returning the revision the write drew.
pub(super) async fn create(store: &PostgresRegistrationStore, id: &Identifier, params: &CreateCsrParams) -> Result<CsourceChangeRevision, RegistrationError> {
    let pattern_forms = stored_pattern_forms(&params.information);
    let operations = operation_codes(&params.operations);
    let dataset_id = dataset_selectors(params.dataset_id.as_deref());
    let scopes = stored_scopes(params.scope.as_deref());
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error(error))?;
    let create_sql = csr_create_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(create_sql.as_str()))
        .bind(id.as_str())
        .bind(params.registration_name.as_ref().map(NonEmptyText::as_str))
        .bind(params.description.as_ref().map(NonEmptyText::as_str))
        .bind(Json(&params.information))
        .bind(dataset_id.as_deref())
        .bind(params.tenant.as_ref().map(TenantName::as_str))
        .bind(params.observation_interval.as_ref().map(Json))
        .bind(params.management_interval.as_ref().map(Json))
        .bind(geometry_bytes(params.location.as_ref().map(Geometry::form)).map_err(|error| persistence_error(error))?)
        .bind(geometry_bytes(params.observation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error(error))?)
        .bind(geometry_bytes(params.operation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error(error))?)
        .bind(params.expires_at)
        .bind(params.endpoint.as_str())
        .bind(params.accept.as_ref().map(ToString::to_string))
        .bind(mode_code(params.mode))
        .bind(operations.as_deref())
        .bind(scopes.label_paths.as_deref())
        .bind(params.refresh_rate.as_ref().map(ToString::to_string))
        .bind(params.management.as_ref().map(Json))
        .bind(params.context_source_info.as_ref().map(Json))
        .bind(params.jsonld_context.as_ref().map(DereferenceableUri::as_str))
        .bind(params.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
        .bind(stored_properties(&params.additional_properties))
        .bind(pattern_forms.as_ref().map(|stored| Json(&stored.forms)))
        .bind(pattern_forms.as_ref().map(|stored| stored.revision.get()))
        .bind(scopes.written.as_deref())
        .execute(&mut *transaction)
        .await
        .map_err(|error| create_error(error, id))?;
    let delivery_state_sql = delivery_state_insert_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(delivery_state_sql.as_str()))
        .bind(id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    replace_registration_information(&mut transaction, id, &params.information, store.attribute_names.as_ref())
        .await
        .map_err(|error| persistence_error(error))?;
    let revision = advance(&mut *transaction).await.map_err(|error| persistence_error(error))?;
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(revision)
}

/// Replace a registration with a resolved value set and rebuild its routing projection in one transaction.
pub(super) async fn update(store: &PostgresRegistrationStore, id: &Identifier, values: &ResolvedCsrUpdate) -> Result<RegistrationMutation, RegistrationError> {
    let pattern_forms = stored_pattern_forms(&values.information);
    let operations = operation_codes(&values.operations);
    let dataset_id = dataset_selectors(values.dataset_id.as_deref());
    let scopes = stored_scopes(values.scope.as_deref());
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error(error))?;
    let update_sql = csr_update_sql();
    let result = sqlx::query(sqlx::AssertSqlSafe(update_sql.as_str()))
        .bind(id.as_str())
        .bind(values.registration_name.as_ref().map(NonEmptyText::as_str))
        .bind(values.description.as_ref().map(NonEmptyText::as_str))
        .bind(Json(&values.information))
        .bind(dataset_id.as_deref())
        .bind(values.tenant.as_ref().map(TenantName::as_str))
        .bind(values.observation_interval.as_ref().map(Json))
        .bind(values.management_interval.as_ref().map(Json))
        .bind(geometry_bytes(values.location.as_ref().map(Geometry::form)).map_err(|error| persistence_error(error))?)
        .bind(geometry_bytes(values.observation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error(error))?)
        .bind(geometry_bytes(values.operation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error(error))?)
        .bind(values.expires_at)
        .bind(values.endpoint.as_str())
        .bind(values.accept.as_ref().map(ToString::to_string))
        .bind(mode_code(values.mode))
        .bind(operations.as_deref())
        .bind(scopes.label_paths.as_deref())
        .bind(values.refresh_rate.as_ref().map(ToString::to_string))
        .bind(values.management.as_ref().map(Json))
        .bind(values.context_source_info.as_ref().map(Json))
        .bind(values.jsonld_context.as_ref().map(DereferenceableUri::as_str))
        .bind(values.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
        .bind(stored_properties(&values.additional_properties))
        .bind(pattern_forms.as_ref().map(|stored| Json(&stored.forms)))
        .bind(pattern_forms.as_ref().map(|stored| stored.revision.get()))
        .bind(scopes.written.as_deref())
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    // Nothing was replaced, so nothing derived from it may be rewritten: a
    // missing registration has no projection rows to speak for.
    if result.rows_affected() == 0 {
        transaction.rollback().await.map_err(|error| persistence_error(error))?;
        return Ok(RegistrationMutation::Missing);
    }
    replace_registration_information(&mut transaction, id, &values.information, store.attribute_names.as_ref())
        .await
        .map_err(|error| persistence_error(error))?;
    let revision = advance(&mut *transaction).await.map_err(|error| persistence_error(error))?;
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(RegistrationMutation::Applied(revision))
}

/// Record one distributed operation on the health members of Table 5.2.9-2.
pub(super) async fn record_forwarding_outcome(
    store: &PostgresRegistrationStore,
    id: &Identifier,
    outcome: ForwardingOutcome,
    at: DateTime<Utc>,
) -> Result<(), RegistrationError> {
    let sql = health_update_sql(outcome);
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id.as_str())
        .bind(at)
        .execute(&store.pool)
        .await
        .map_err(|error| persistence_error(error))?;
    Ok(())
}

/// Delete a registration and its routing projection rows in one transaction.
pub(super) async fn delete(store: &PostgresRegistrationStore, id: &Identifier) -> Result<RegistrationMutation, RegistrationError> {
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error(error))?;
    // Removed before the payload so the deletion holds whatever referential
    // action the foreign key carries; a registration that lingers in the
    // projection keeps being preselected for distributed operations.
    let delete_information = Query::delete()
        .from_table(RegistrationInformation::Table)
        .and_where(SeaExpr::col(RegistrationInformation::RegistrationId).eq(id.as_str()))
        .to_owned();
    let (info_sql, info_values) = delete_information.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(info_sql.as_str()), info_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;

    let delete_registration = Query::delete()
        .from_table(Csr::Table)
        .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
        .to_owned();
    let (delete_sql, delete_values) = delete_registration.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error(error))?;
    if result.rows_affected() == 0 {
        transaction.rollback().await.map_err(|error| persistence_error(error))?;
        return Ok(RegistrationMutation::Missing);
    }
    let revision = advance(&mut *transaction).await.map_err(|error| persistence_error(error))?;
    transaction.commit().await.map_err(|error| persistence_error(error))?;
    Ok(RegistrationMutation::Applied(revision))
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_write::stored_properties;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use indexmap::IndexMap;

    /// A registration without Context Source Properties stores `NULL`, not an
    /// empty object, which the `q` pushdown reads as having none.
    #[test]
    fn a_registration_without_properties_stores_null() {
        assert!(stored_properties(&IndexMap::new()).is_none());
    }

    /// The properties are stored as the JSON object they were written as, in
    /// the order written.
    #[test]
    fn the_properties_are_stored_as_the_object_they_were_written_as() {
        let properties: IndexMap<AttributeName, JsonLiteral> = [
            (
                "https://example.org/zeta".parse().expect("a valid name"),
                JsonLiteral::admit("1").expect("a JSON literal"),
            ),
            (
                "https://example.org/alpha".parse().expect("a valid name"),
                JsonLiteral::admit(r#"{"type": "Property", "value": "x"}"#).expect("a JSON literal"),
            ),
        ]
        .into_iter()
        .collect();

        let stored = stored_properties(&properties).expect("properties are stored");

        pretty_assertions::assert_eq!(
            serde_json::to_string(&stored.0).expect("the properties write"),
            r#"{"https://example.org/zeta":1,"https://example.org/alpha":{"type":"Property","value":"x"}}"#
        );
    }
}
