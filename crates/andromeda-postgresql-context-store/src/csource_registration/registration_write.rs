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
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
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
use sea_query::{Expr as SeaExpr, ExprTrait, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;

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
    let information = serde_json::to_value(&params.information)?;
    let pattern_forms = stored_pattern_forms(&params.information);
    let observation_interval = params.observation_interval.map(serde_json::to_value).transpose()?;
    let management_interval = params.management_interval.map(serde_json::to_value).transpose()?;
    let management = params.management.as_ref().map(serde_json::to_value).transpose()?;
    let context_source_info = params.context_source_info.as_ref().map(serde_json::to_value).transpose()?;
    let operations = operation_codes(&params.operations);
    let dataset_id = dataset_selectors(params.dataset_id.as_deref());
    let scopes = stored_scopes(params.scope.as_deref());
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error(error))?;
    let create_sql = csr_create_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(create_sql.as_str()))
        .bind(id.as_str())
        .bind(params.registration_name.as_ref().map(NonEmptyText::as_str))
        .bind(params.description.as_ref().map(NonEmptyText::as_str))
        .bind(information)
        .bind(dataset_id.as_deref())
        .bind(params.tenant.as_ref().map(TenantName::as_str))
        .bind(observation_interval)
        .bind(management_interval)
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
        .bind(management)
        .bind(context_source_info)
        .bind(params.jsonld_context.as_ref().map(DereferenceableUri::as_str))
        .bind(params.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
        .bind(&params.additional_properties)
        .bind(pattern_forms.as_ref().map(|stored| &stored.forms))
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
    let information = serde_json::to_value(&values.information)?;
    let pattern_forms = stored_pattern_forms(&values.information);
    let observation_interval = values.observation_interval.map(serde_json::to_value).transpose()?;
    let management_interval = values.management_interval.map(serde_json::to_value).transpose()?;
    let management = values.management.as_ref().map(serde_json::to_value).transpose()?;
    let context_source_info = values.context_source_info.as_ref().map(serde_json::to_value).transpose()?;
    let operations = operation_codes(&values.operations);
    let dataset_id = dataset_selectors(values.dataset_id.as_deref());
    let scopes = stored_scopes(values.scope.as_deref());
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error(error))?;
    let update_sql = csr_update_sql();
    let result = sqlx::query(sqlx::AssertSqlSafe(update_sql.as_str()))
        .bind(id.as_str())
        .bind(values.registration_name.as_ref().map(NonEmptyText::as_str))
        .bind(values.description.as_ref().map(NonEmptyText::as_str))
        .bind(information)
        .bind(dataset_id.as_deref())
        .bind(values.tenant.as_ref().map(TenantName::as_str))
        .bind(observation_interval)
        .bind(management_interval)
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
        .bind(management)
        .bind(context_source_info)
        .bind(values.jsonld_context.as_ref().map(DereferenceableUri::as_str))
        .bind(values.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
        .bind(&values.additional_properties)
        .bind(pattern_forms.as_ref().map(|stored| &stored.forms))
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
