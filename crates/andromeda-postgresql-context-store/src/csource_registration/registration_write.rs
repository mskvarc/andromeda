//! The write side of the registration persistence port: every mutation
//! rewrites the registration's information rows in the same transaction as
//! its registration row.

use crate::{
    context_source_change_clock::advance,
    csource_registration::{
        information_write::{prepare_information, replace_registration_information},
        registration_columns::{RegistrationFields, registration_columns},
        registration_read::{expired_select, registrations_of, written},
        registration_statements::{csr_create_statement, csr_update_statement, delivery_state_insert, health_update},
        registration_store::{PostgresRegistrationStore, create_error},
    },
    store_fault::persistence_error,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_postgresql_seaquery::{
    external_arguments::{ExternalArguments, ExternallyBound},
    statement::Statement,
};
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
use sea_query::{Expr as SeaExpr, ExprTrait, LockType, Query, SelectStatement};

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
    let mut transaction = store
        .clock_writers
        .begin()
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    // The rows to return are read before either deletion, information rows
    // included; the two deletions then remove exactly the same set, agreeing
    // without depending on the foreign key's referential action. All three run
    // in one transaction, so no reader observes a partial removal.
    let rows = swept_select(cutoff)
        .to_query()
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
    delete_information
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;

    let delete_registrations = Query::delete()
        .from_table(Csr::Table)
        .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
        .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff))
        .to_owned();
    delete_registrations
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;

    let revision = advance(&mut transaction).await.map_err(|error| persistence_error::<RegistrationError>(error))?;
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
    let mut transaction = store
        .clock_writers
        .begin()
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let information = prepare_information(&mut transaction, &params.information, store.attribute_names.as_ref())
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let mut external = ExternalArguments::default();
    let columns = registration_columns(&RegistrationFields::from(params), information.pattern_forms_revision(), &mut external)?;
    ExternallyBound::new(csr_create_statement(id, columns)?, external)
        .into_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| create_error(error, id))?;
    delivery_state_insert(id)?
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    replace_registration_information(&mut transaction, id, &information)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let registration = written(&mut transaction, id, &information.key_names()).await?;
    let revision = advance(&mut transaction).await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(WrittenRegistration { revision, registration })
}

/// Replace a registration with a resolved value set and its information rows in one transaction, returning the registration as written.
pub(super) async fn update(
    store: &PostgresRegistrationStore,
    id: &Identifier,
    values: &ResolvedCsrUpdate,
) -> Result<RegistrationMutation<WrittenRegistration>, RegistrationError> {
    let mut transaction = store
        .clock_writers
        .begin()
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let information = prepare_information(&mut transaction, &values.information, store.attribute_names.as_ref())
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    let mut external = ExternalArguments::default();
    let columns = registration_columns(&RegistrationFields::from(values), information.pattern_forms_revision(), &mut external)?;
    let result = ExternallyBound::new(csr_update_statement(id, columns), external)
        .into_query()
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
    let revision = advance(&mut transaction).await.map_err(|error| persistence_error::<RegistrationError>(error))?;
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
    health_update(id, outcome, at)
        .to_query()
        .execute(&store.pool)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(())
}

/// Delete a registration and its information rows in one transaction.
pub(super) async fn delete(store: &PostgresRegistrationStore, id: &Identifier) -> Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError> {
    let mut transaction = store
        .clock_writers
        .begin()
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    // Removed before the registration row, so the deletion holds whatever
    // referential action the foreign key carries.
    let delete_information = Query::delete()
        .from_table(RegistrationInformation::Table)
        .and_where(SeaExpr::col(RegistrationInformation::RegistrationId).eq(id.as_str()))
        .to_owned();
    delete_information
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;

    let delete_registration = Query::delete()
        .from_table(Csr::Table)
        .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
        .to_owned();
    let result = delete_registration
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<RegistrationError>(error))?;
    if result.rows_affected() == 0 {
        transaction.rollback().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
        return Ok(RegistrationMutation::Missing);
    }
    let revision = advance(&mut transaction).await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    transaction.commit().await.map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(RegistrationMutation::Applied(revision))
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_write::swept_select;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use sea_query::PostgresQueryBuilder;

    /// The registration rows are locked, not the delivery state they are
    /// joined to: `FOR UPDATE` cannot lock the nullable side of an outer join.
    #[test]
    fn the_sweep_locks_the_registration_rows_it_reads() {
        let cutoff = "2026-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let sql = swept_select(cutoff).to_string(PostgresQueryBuilder);
        assert!(sql.ends_with("FOR UPDATE OF \"context_source_registrations\""), "{sql}");
    }
}
