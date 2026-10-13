//! Applying queued temporal append intents to the live branch's history in
//! SQL.
//!
//! An intent waits in `temporal_append_intents` with its entries already in the
//! shape of the history rows they become (`temporal_entry`), so an application
//! is a fixed sequence of statements that read the claimed rows where they
//! wait: the write records, the Temporal Evolutions and streams, the
//! instance replacements and claims, the history and Scope rows. Nothing is
//! read back into the broker. The caller runs the sequence in the transaction
//! that also removes the intents, so a reader sees each entry queued or
//! recorded, never both.

/// The write records and catalogue an application establishes first.
mod catalog_statements;
/// The history and Scope rows and the instance claims an application records.
mod history_statements;
/// The claimed entries every statement reads.
mod queued_entries;

use crate::{
    duplicate_instance_claim::claim_write_failure,
    error::TemporalStoreError,
    intent_application::{
        catalog_statements::{evolutions_statement, streams_statement, untyped_observations_statement, write_records_statement},
        history_statements::{
            claims_statement,
            history_statement,
            released_claims_statement,
            replaced_instances_statement,
            replaced_scopes_statement,
            scope_history_statement,
        },
        queued_entries::rendered,
    },
    stored_catalog_error::StoredCatalogError,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_temporal::temporal_not_found_reason::TemporalNotFoundReason;
use sea_query_sqlx::{SqlxBinder, SqlxValues};
use sqlx::{AssertSqlSafe, PgConnection, Row};

/// What the application does with one statement's outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepRole {
    /// Run it.
    Write,
    /// Run it and refuse the application when it returns a row.
    RefuseUntyped,
    /// Run it and refuse a duplicate `instanceId` claim as the client's own.
    Claim,
}

/// One statement of the application, rendered with its values.
struct Step {
    /// What its outcome means.
    role: StepRole,
    /// The statement text.
    sql: String,
    /// Its values.
    values: SqlxValues,
}

impl Step {
    /// `statement` in `role`.
    fn of(role: StepRole, statement: &impl SqlxBinder) -> Step {
        let (sql, values) = rendered(statement);
        Step { role, sql, values }
    }
}

/// The statements applying the queued intents named by `keys`, in the order
/// they run: the Temporal Evolutions and streams before the history that
/// references them, the refusal of an untyped observation after the
/// evolutions the batch itself creates, the instance replacements before the
/// claims and rows that take their place.
fn application_steps(keys: &[i64]) -> Result<Vec<Step>, TemporalStoreError> {
    Ok(vec![
        Step::of(StepRole::Write, &write_records_statement(keys).map_err(TemporalStoreError::StatementBuild)?),
        Step::of(StepRole::Write, &evolutions_statement(keys).map_err(TemporalStoreError::StatementBuild)?),
        Step::of(StepRole::RefuseUntyped, &untyped_observations_statement(keys)),
        Step::of(StepRole::Write, &streams_statement(keys).map_err(TemporalStoreError::StatementBuild)?),
        Step::of(StepRole::Write, &replaced_instances_statement(keys)),
        Step::of(StepRole::Write, &released_claims_statement(keys)),
        Step::of(StepRole::Claim, &claims_statement(keys).map_err(TemporalStoreError::StatementBuild)?),
        Step::of(StepRole::Write, &history_statement(keys).map_err(TemporalStoreError::StatementBuild)?),
        Step::of(StepRole::Write, &replaced_scopes_statement(keys)),
        Step::of(StepRole::Write, &scope_history_statement(keys).map_err(TemporalStoreError::StatementBuild)?),
    ])
}

/// Apply the queued intents named by `keys` to the live branch's history, in
/// the caller's transaction.
///
/// An intent that observes an Entity with no Temporal Evolution and states no
/// type to create one refuses the whole application: clause 4.5.1 makes at
/// least one type part of an Entity, and nothing may invent one.
///
/// # Errors
///
/// Returns [`TemporalStoreError::NotFound`] naming the Entity of the first such
/// intent, [`TemporalStoreError::DuplicateInstanceClaim`] when two instances of
/// one attribute claim one `instanceId`, [`TemporalStoreError::StatementBuild`]
/// when a statement cannot be built, and [`TemporalStoreError::Database`] when
/// one fails.
pub async fn apply_queued_intents_in_tx(connection: &mut PgConnection, keys: &[i64]) -> Result<(), TemporalStoreError> {
    if keys.is_empty() {
        return Ok(());
    }
    for Step { role, sql, values } in application_steps(keys)? {
        let query = sqlx::query_with(AssertSqlSafe(sql), values);
        match role {
            StepRole::Write => {
                query.execute(&mut *connection).await?;
            }
            StepRole::Claim => {
                query.execute(&mut *connection).await.map_err(claim_write_failure)?;
            }
            StepRole::RefuseUntyped => {
                if let Some(row) = query.fetch_optional(&mut *connection).await? {
                    let entity_id: String = row.try_get(1)?;
                    return Err(TemporalStoreError::NotFound(TemporalNotFoundReason::UntypedEvolution {
                        entity: entity_id.parse::<Identifier>().map_err(StoredCatalogError::EntityId)?,
                    }));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
