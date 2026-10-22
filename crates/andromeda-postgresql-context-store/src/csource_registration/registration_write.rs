//! The write side of the registration persistence port: every mutation
//! rewrites the registration's information rows in the same transaction as
//! its registration row.

use crate::{
    context_source_change_clock::advance,
    csource_registration::{
        information_write::{prepare_information, replace_registration_information},
        registration_read::{expired_select, registrations_of, written},
        registration_statements::{
            ManagementColumns,
            context_source_info_columns,
            csr_create_sql,
            csr_update_sql,
            dataset_selectors,
            delivery_state_insert_sql,
            health_update_sql,
            interval_columns,
            mode_code,
            operation_codes,
            stored_scopes,
        },
        registration_store::{PostgresRegistrationStore, create_error},
    },
    store_fault::persistence_error,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    identifier::Identifier,
    non_empty_text::NonEmptyText,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use andromeda_ngsi_ld_query::portable_pattern::RenderingRevision;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{
        create_params::CreateCsrParams,
        expired_deletion::ExpiredDeletion,
        forwarding_outcome::ForwardingOutcome,
        mutation::RegistrationMutation,
        written_registration::WrittenRegistration,
    },
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use sea_query::{Expr as SeaExpr, ExprTrait, LockType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::types::Json;

/// The `additional_properties` column value: the Context Source Properties of
/// a registration, or `NULL` for a registration that has none.
fn stored_properties(properties: &IndexMap<AttributeName, JsonLiteral>) -> Option<Json<&IndexMap<AttributeName, JsonLiteral>>> {
    (!properties.is_empty()).then_some(Json(properties))
}

/// The registrations the expiry sweep deletes, locked as they are read.
///
/// A concurrent deletion or update of one of them either committed before the
/// lock, so the row is not returned or is read as it left it, or waits behind
/// it until the sweep has deleted it, so the sweep returns each registration
/// exactly as it deletes it.
fn swept_select(cutoff: DateTime<Utc>) -> SelectStatement {
    let mut select = expired_select(cutoff);
    select.lock_with_tables(LockType::Update, [Csr::Table]);
    select
}

/// Delete every registration due at or before `cutoff`, returning the deleted rows and the revision the sweep drew; their information rows go in the same transaction.
pub(super) async fn delete_expired(store: &PostgresRegistrationStore, cutoff: DateTime<Utc>) -> Result<ExpiredDeletion, RegistrationError> {
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    // The rows to return are read before either deletion, information rows
    // included; the two deletions then remove exactly the same set, agreeing
    // without depending on the foreign key's referential action. All three run
    // in one transaction, so no reader observes a partial removal.
    let (select_sql, select_values) = swept_select(cutoff).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(select_sql.as_str()), select_values)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let expired = registrations_of(store.attribute_names.as_ref(), &rows).await?;

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
        .map_err(|error| persistence_error::<RegistrationError>(error))?;

    let delete_registrations = Query::delete()
        .from_table(Csr::Table)
        .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
        .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff))
        .to_owned();
    let (delete_sql, delete_values) = delete_registrations.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;

    let revision = advance(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(ExpiredDeletion {
        revision,
        registrations: expired,
    })
}

/// Persist a new registration, its delivery state and its information rows
/// in one transaction, returning the registration as written and the revision
/// the write drew.
pub(super) async fn create(store: &PostgresRegistrationStore, id: &Identifier, params: &CreateCsrParams) -> Result<WrittenRegistration, RegistrationError> {
    let operations = operation_codes(&params.operations);
    let dataset_id = dataset_selectors(params.dataset_id.as_deref());
    let scopes = stored_scopes(params.scope.as_deref());
    let (observation_start, observation_end) = interval_columns(params.observation_interval.as_ref());
    let (management_start, management_end) = interval_columns(params.management_interval.as_ref());
    let management = ManagementColumns::of(params.management.as_ref());
    let (info_keys, info_values) = context_source_info_columns(params.context_source_info.as_deref());
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    let information = prepare_information(&mut transaction, &params.information, store.attribute_names.as_ref())
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let create_sql = csr_create_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(create_sql.as_str()))
        .bind(id.as_str())
        .bind(params.registration_name.as_ref().map(NonEmptyText::as_str))
        .bind(params.description.as_ref().map(NonEmptyText::as_str))
        .bind(dataset_id.as_deref())
        .bind(params.tenant.as_ref().map(TenantName::as_str))
        .bind(observation_start)
        .bind(observation_end)
        .bind(management_start)
        .bind(management_end)
        .bind(geometry_bytes(params.location.as_ref().map(Geometry::form)).map_err(|error| persistence_error::<RegistrationError>(error))?)
        .bind(geometry_bytes(params.observation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error::<RegistrationError>(error))?)
        .bind(geometry_bytes(params.operation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error::<RegistrationError>(error))?)
        .bind(params.expires_at)
        .bind(params.endpoint.as_str())
        .bind(params.accept.as_ref().map(ToString::to_string))
        .bind(mode_code(params.mode))
        .bind(operations.as_deref())
        .bind(scopes.label_paths.as_deref())
        .bind(params.refresh_rate.as_ref().map(ToString::to_string))
        .bind(management.declared)
        .bind(management.cache_duration.as_deref())
        .bind(management.cooldown)
        .bind(management.local_only)
        .bind(management.timeout)
        .bind(info_keys.as_deref())
        .bind(info_values.as_deref())
        .bind(params.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
        .bind(stored_properties(&params.additional_properties))
        .bind(information.pattern_forms_revision().map(RenderingRevision::get))
        .bind(scopes.written.as_deref())
        .execute(&mut *transaction)
        .await
        .map_err(|error| create_error(error, id))?;
    let delivery_state_sql = delivery_state_insert_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(delivery_state_sql.as_str()))
        .bind(id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    replace_registration_information(&mut transaction, id, &information)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let registration = written(&mut transaction, id, &information.key_names()).await?;
    let revision = advance(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(WrittenRegistration { revision, registration })
}

/// Replace a registration with a resolved value set and its information rows in one transaction, returning the registration as written.
pub(super) async fn update(
    store: &PostgresRegistrationStore,
    id: &Identifier,
    values: &ResolvedCsrUpdate,
) -> Result<RegistrationMutation<WrittenRegistration>, RegistrationError> {
    let operations = operation_codes(&values.operations);
    let dataset_id = dataset_selectors(values.dataset_id.as_deref());
    let scopes = stored_scopes(values.scope.as_deref());
    let (observation_start, observation_end) = interval_columns(values.observation_interval.as_ref());
    let (management_start, management_end) = interval_columns(values.management_interval.as_ref());
    let management = ManagementColumns::of(values.management.as_ref());
    let (info_keys, info_values) = context_source_info_columns(values.context_source_info.as_deref());
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    let information = prepare_information(&mut transaction, &values.information, store.attribute_names.as_ref())
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let update_sql = csr_update_sql();
    let result = sqlx::query(sqlx::AssertSqlSafe(update_sql.as_str()))
        .bind(id.as_str())
        .bind(values.registration_name.as_ref().map(NonEmptyText::as_str))
        .bind(values.description.as_ref().map(NonEmptyText::as_str))
        .bind(dataset_id.as_deref())
        .bind(values.tenant.as_ref().map(TenantName::as_str))
        .bind(observation_start)
        .bind(observation_end)
        .bind(management_start)
        .bind(management_end)
        .bind(geometry_bytes(values.location.as_ref().map(Geometry::form)).map_err(|error| persistence_error::<RegistrationError>(error))?)
        .bind(geometry_bytes(values.observation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error::<RegistrationError>(error))?)
        .bind(geometry_bytes(values.operation_space.as_ref().map(Geometry::form)).map_err(|error| persistence_error::<RegistrationError>(error))?)
        .bind(values.expires_at)
        .bind(values.endpoint.as_str())
        .bind(values.accept.as_ref().map(ToString::to_string))
        .bind(mode_code(values.mode))
        .bind(operations.as_deref())
        .bind(scopes.label_paths.as_deref())
        .bind(values.refresh_rate.as_ref().map(ToString::to_string))
        .bind(management.declared)
        .bind(management.cache_duration.as_deref())
        .bind(management.cooldown)
        .bind(management.local_only)
        .bind(management.timeout)
        .bind(info_keys.as_deref())
        .bind(info_values.as_deref())
        .bind(values.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
        .bind(stored_properties(&values.additional_properties))
        .bind(information.pattern_forms_revision().map(RenderingRevision::get))
        .bind(scopes.written.as_deref())
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    // A missing registration has no information rows to replace, and the
    // names interned for it roll back with the transaction.
    if result.rows_affected() == 0 {
        transaction.rollback().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
        return Ok(RegistrationMutation::Missing);
    }
    replace_registration_information(&mut transaction, id, &information)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let registration = written(&mut transaction, id, &information.key_names()).await?;
    let revision = advance(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(RegistrationMutation::Applied(WrittenRegistration { revision, registration }))
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
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(())
}

/// Delete a registration and its information rows in one transaction.
pub(super) async fn delete(store: &PostgresRegistrationStore, id: &Identifier) -> Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError> {
    let mut transaction = store.pool.begin().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    // Removed before the registration row, so the deletion holds whatever
    // referential action the foreign key carries.
    let delete_information = Query::delete()
        .from_table(RegistrationInformation::Table)
        .and_where(SeaExpr::col(RegistrationInformation::RegistrationId).eq(id.as_str()))
        .to_owned();
    let (info_sql, info_values) = delete_information.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(info_sql.as_str()), info_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;

    let delete_registration = Query::delete()
        .from_table(Csr::Table)
        .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
        .to_owned();
    let (delete_sql, delete_values) = delete_registration.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    if result.rows_affected() == 0 {
        transaction.rollback().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
        return Ok(RegistrationMutation::Missing);
    }
    let revision = advance(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(RegistrationMutation::Applied(revision))
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_write::{stored_properties, swept_select};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::date_time::NgsiLdDateTime};
    use indexmap::IndexMap;
    use sea_query::PostgresQueryBuilder;

    /// The registration rows are locked, not the delivery state they are
    /// joined to: `FOR UPDATE` cannot lock the nullable side of an outer join.
    #[test]
    fn the_sweep_locks_the_registration_rows_it_reads() {
        let cutoff = "2026-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let sql = swept_select(cutoff).to_string(PostgresQueryBuilder);
        assert!(sql.ends_with("FOR UPDATE OF \"context_source_registrations\""), "{sql}");
    }

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
