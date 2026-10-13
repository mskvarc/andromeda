//! Applying temporal append intents to a branch's history in SQL.
//!
//! An intent's entries are already in the shape of the history rows they
//! become (`temporal_entry`), so an application is a fixed sequence of
//! statements that read the intents where they are: the claimed rows of the
//! queue for the drain, the commit's own intents bound as one array for a
//! commit that records natively. The statements write the write records, the
//! Temporal Evolutions and streams, the instance replacements and claims, the
//! history and Scope rows; nothing is read back into the broker. The drain
//! runs the sequence in the transaction that also removes the intents, so a
//! reader sees each entry queued or recorded, never both.

/// The write records and catalogue an application establishes first.
mod catalog_statements;
/// The history and Scope rows and the instance claims an application records.
mod history_statements;
/// The intents an application reads, and their entries.
pub mod intent_source;

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
        intent_source::{EntryNeed, IntentSource, rendered},
    },
    stored_catalog_error::StoredCatalogError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::schema_iden;
use andromeda_temporal::temporal_not_found_reason::TemporalNotFoundReason;
use sea_query::{DynIden, QueryStatementWriter};
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

/// One statement of the application, rendered.
struct Step {
    /// What its outcome means.
    role: StepRole,
    /// What the source has to hold for it to change anything.
    need: EntryNeed,
    /// The statement text.
    sql: String,
}

impl Step {
    /// `statement` in `role`, changing something only where the source meets
    /// `need`.
    fn of(role: StepRole, need: EntryNeed, statement: &impl QueryStatementWriter) -> Step {
        Step {
            role,
            need,
            sql: rendered(statement),
        }
    }
}

/// The statements applying the intents of `source` to the branch `schema`
/// names, in the order they run: the Temporal Evolutions and streams before
/// the history that references them, the refusal of an untyped observation
/// after the evolutions the source itself creates, the instance replacements
/// before the claims and rows that take their place.
fn application_steps(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<Vec<Step>, TemporalStoreError> {
    let build = TemporalStoreError::StatementBuild;
    Ok(vec![
        Step::of(StepRole::Write, EntryNeed::Any, &write_records_statement(source).map_err(build)?),
        Step::of(StepRole::Write, EntryNeed::TypedIntent, &evolutions_statement(source, schema).map_err(build)?),
        Step::of(
            StepRole::RefuseUntyped,
            EntryNeed::UntypedObservation,
            &untyped_observations_statement(source, schema),
        ),
        Step::of(StepRole::Write, EntryNeed::AttributeEntry, &streams_statement(source, schema).map_err(build)?),
        Step::of(StepRole::Write, EntryNeed::VerbatimInstance, &replaced_instances_statement(source, schema)),
        Step::of(StepRole::Write, EntryNeed::VerbatimInstance, &released_claims_statement(source, schema)),
        Step::of(StepRole::Claim, EntryNeed::VerbatimInstance, &claims_statement(source, schema).map_err(build)?),
        Step::of(StepRole::Write, EntryNeed::AttributeEntry, &history_statement(source, schema).map_err(build)?),
        Step::of(StepRole::Write, EntryNeed::ScopeEntry, &replaced_scopes_statement(source, schema)),
        Step::of(StepRole::Write, EntryNeed::ScopeEntry, &scope_history_statement(source, schema).map_err(build)?),
    ])
}

/// Apply the intents of `source` to the history of `target`, in the caller's
/// transaction.
///
/// A statement the source's entries cannot reach is not run. An intent that
/// observes an Entity with no Temporal Evolution and states no type to create
/// one refuses the whole application: clause 4.5.1 makes at least one type
/// part of an Entity, and nothing may invent one.
///
/// # Errors
///
/// Returns [`TemporalStoreError::NotFound`] naming the Entity of the first such
/// intent, [`TemporalStoreError::DuplicateInstanceClaim`] when two instances of
/// one attribute claim one `instanceId`, [`TemporalStoreError::StatementBuild`]
/// when a statement cannot be built, and [`TemporalStoreError::Database`] when
/// one fails.
pub async fn apply_intents_in_tx(connection: &mut PgConnection, source: &IntentSource<'_>, target: &ContextTarget) -> Result<(), TemporalStoreError> {
    let schema = schema_iden(target);
    for step in application_steps(source, schema.as_ref())? {
        if source.may_meet(step.need) {
            run(connection, source, step).await?;
        }
    }
    Ok(())
}

/// Record the attribute history of `source` alone in the relations of
/// `schema`, whose evolutions and streams the caller has written.
///
/// # Errors
///
/// Returns [`TemporalStoreError::StatementBuild`] when the statement cannot be
/// built and [`TemporalStoreError::Database`] when it fails.
pub(crate) async fn record_history_in_tx(connection: &mut PgConnection, source: &IntentSource<'_>, schema: &DynIden) -> Result<(), TemporalStoreError> {
    let statement = history_statement(source, Some(schema)).map_err(TemporalStoreError::StatementBuild)?;
    run(connection, source, Step::of(StepRole::Write, EntryNeed::AttributeEntry, &statement)).await
}

/// Run one step over `source`.
async fn run(connection: &mut PgConnection, source: &IntentSource<'_>, Step { role, sql, .. }: Step) -> Result<(), TemporalStoreError> {
    let query = source.bind(sqlx::query(AssertSqlSafe(sql)));
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
    Ok(())
}

#[cfg(test)]
mod tests;
