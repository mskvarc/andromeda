//! Applying temporal append intents to a branch's history in SQL.
//!
//! An intent's entries are already in the shape of the history rows they
//! become (`temporal_entry`), so an application is a fixed sequence of
//! statements that read the intents where they are: the claimed rows of the
//! queue for the drain, the commit's own intents bound as one array for a
//! commit that records natively. The statements write the write records, the
//! Temporal Evolutions and streams, the instance replacements and claims, the
//! history rows, the stream extents covering their geometries and the stream
//! shapes their values carry, and the Scope rows; nothing is read back into
//! the broker beyond the keys of the evolutions the application created, which
//! the statements after them are sent, and the rows a replacement removes. The
//! drain runs the sequence in the transaction that also removes the intents, so a
//! reader sees each entry queued or recorded, never both.

/// The write records and catalogue an application establishes first.
mod catalog_statements;
/// Which ends of an instance an application records.
mod deletion_judgement;
/// The Temporal Evolutions an application writes, and their type memberships.
mod evolution_upsert;
/// The history and Scope rows, the stream extents and shapes and the instance
/// claims an application records.
mod history_statements;
/// The intents an application reads, and their entries.
pub mod intent_source;
/// Removing the history rows an application's entries replace.
mod replaced_rows;
/// Which Scope entries an application records.
mod scope_judgement;

use crate::{
    duplicate_instance_claim::claim_write_failure,
    error::TemporalStoreError,
    intent_application::{
        catalog_statements::{evolutions_and_streams_statement, untyped_observations_statement, untyped_streams_statement, write_records_statement},
        history_statements::{
            claims_statement,
            extent_growth_statement,
            history_statement,
            released_claims_statement,
            replaced_instances_statement,
            scope_history_statement,
            stream_shapes_statement,
        },
        intent_source::{EntryReads, IntentSource, rendered},
        replaced_rows::remove_replaced,
    },
    stored_catalog_error::StoredCatalogError,
    stream_shape_pruning::prune_stream_shapes,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
use andromeda_temporal::temporal_not_found_reason::TemporalNotFoundReason;
use sea_query::{DynIden, QueryStatementWriter};
use sqlx::{AssertSqlSafe, PgConnection, Row};

/// What the application does with one statement's outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepRole {
    /// Run it.
    Write,
    /// Run it and keep the keys of the Temporal Evolutions it returns as
    /// created.
    Evolve,
    /// Run it and refuse the application when it returns a row.
    RefuseUntyped,
    /// Run it and refuse a duplicate `instanceId` claim as the client's own.
    Claim,
    /// Read the attribute history rows the entries replace, then remove
    /// exactly those rows and prune their streams' shapes.
    Replace,
}

/// Whether a statement reads the keys of the Temporal Evolutions the
/// application created, bound after the source's own values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreatedKeys {
    /// It does not.
    Unread,
    /// It does.
    Read,
}

/// One statement of the application, rendered.
struct Step {
    /// What its outcome means.
    role: StepRole,
    /// What the source has to hold for it to change anything; `None` when it
    /// runs for every source.
    need: Option<TemporalIntentReach>,
    /// What of the entries it reads.
    reads: EntryReads,
    /// Whether it reads the keys of the evolutions the application created.
    created: CreatedKeys,
    /// The statement text.
    sql: String,
}

impl Step {
    /// `statement` in `role`, reading `reads` of the entries and changing
    /// something only where the source meets `need`.
    fn of(role: StepRole, need: Option<TemporalIntentReach>, reads: EntryReads, statement: &impl QueryStatementWriter) -> Step {
        Step {
            role,
            need,
            reads,
            created: CreatedKeys::Unread,
            sql: rendered(statement),
        }
    }

    /// The step also reading the keys of the evolutions the application
    /// created.
    fn reading_created(self) -> Step {
        Step {
            created: CreatedKeys::Read,
            ..self
        }
    }
}

/// The statements applying the intents of `source` to the branch `schema`
/// names, in the order they run: the Temporal Evolutions and streams before
/// the history that references them, the refusal of an untyped observation
/// after the evolutions the source itself creates, the streams of the intents
/// stating no type after that refusal, the instance replacements before the
/// claims and rows that take their place, the stream extents and shapes after
/// the history rows whose geometries and values they cover. Every statement
/// reading attribute entries after the evolutions are written reads the keys
/// of the ones the application created, which decide the ends of an instance
/// it records.
fn application_steps(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<Vec<Step>, TemporalStoreError> {
    let build = TemporalStoreError::StatementBuild;
    let (nothing, identities, everything) = (EntryReads::Nothing, EntryReads::Identities, EntryReads::Everything);
    Ok(vec![
        Step::of(StepRole::Write, None, nothing, &write_records_statement(source).map_err(build)?),
        Step::of(
            StepRole::Evolve,
            Some(TemporalIntentReach::TypedIntent),
            identities,
            &evolutions_and_streams_statement(source, schema).map_err(build)?,
        ),
        Step::of(
            StepRole::RefuseUntyped,
            Some(TemporalIntentReach::UntypedObservation),
            identities,
            &untyped_observations_statement(source, schema),
        ),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::UntypedAttributeEntry),
            identities,
            &untyped_streams_statement(source, schema).map_err(build)?,
        )
        .reading_created(),
        Step::of(
            StepRole::Replace,
            Some(TemporalIntentReach::VerbatimInstance),
            identities,
            &replaced_instances_statement(source, schema),
        )
        .reading_created(),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::VerbatimInstance),
            identities,
            &released_claims_statement(source, schema),
        )
        .reading_created(),
        Step::of(
            StepRole::Claim,
            Some(TemporalIntentReach::VerbatimInstance),
            identities,
            &claims_statement(source, schema).map_err(build)?,
        )
        .reading_created(),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::AttributeEntry),
            everything,
            &history_statement(source, schema).map_err(build)?,
        )
        .reading_created(),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::AttributeEntry),
            everything,
            &extent_growth_statement(source, schema),
        )
        .reading_created(),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::AttributeEntry),
            everything,
            &stream_shapes_statement(source, schema).map_err(build)?,
        )
        .reading_created(),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::ScopeEntry),
            everything,
            &scope_history_statement(source, schema).map_err(build)?,
        ),
    ])
}

/// Apply the intents of `source` to the history of `target`, in the caller's
/// transaction.
///
/// A statement no intent of the source reaches is not run, and each
/// statement of a bound source is sent only the fields of the entries it
/// reads. An intent that
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
    let mut created = Vec::new();
    for step in application_steps(source, schema.as_ref())? {
        if step.need.is_none_or(|need| source.may_meet(need))
            && let Some(keys) = run(connection, source, schema.as_ref(), &created, step).await?
        {
            created = keys;
        }
    }
    Ok(())
}

/// Record the attribute history of `source` alone in the relations of
/// `schema`, whose evolutions and streams the caller has written, so the
/// application creates none.
///
/// # Errors
///
/// Returns [`TemporalStoreError::StatementBuild`] when the statement cannot be
/// built and [`TemporalStoreError::Database`] when it fails.
pub(crate) async fn record_history_in_tx(connection: &mut PgConnection, source: &IntentSource<'_>, schema: &DynIden) -> Result<(), TemporalStoreError> {
    let statement = history_statement(source, Some(schema)).map_err(TemporalStoreError::StatementBuild)?;
    run(
        connection,
        source,
        Some(schema),
        &[],
        Step::of(StepRole::Write, Some(TemporalIntentReach::AttributeEntry), EntryReads::Everything, &statement).reading_created(),
    )
    .await?;
    Ok(())
}

/// Run one step over `source` in the branch `schema` names, `created` the keys
/// of the evolutions the application created, and return the keys an
/// [`StepRole::Evolve`] step created.
async fn run(
    connection: &mut PgConnection,
    source: &IntentSource<'_>,
    schema: Option<&DynIden>,
    created: &[i64],
    Step {
        role,
        reads,
        created: created_keys,
        sql,
        ..
    }: Step,
) -> Result<Option<Vec<i64>>, TemporalStoreError> {
    let query = source.bind(sqlx::query(AssertSqlSafe(sql)), reads);
    let query = match created_keys {
        CreatedKeys::Unread => query,
        CreatedKeys::Read => query.bind(created),
    };
    match role {
        StepRole::Write => {
            query.execute(&mut *connection).await?;
        }
        StepRole::Evolve => {
            let rows = query.fetch_all(&mut *connection).await?;
            return Ok(Some(rows.iter().map(|row| row.try_get(0)).collect::<Result<_, _>>()?));
        }
        StepRole::Claim => {
            query.execute(&mut *connection).await.map_err(claim_write_failure)?;
        }
        StepRole::Replace => {
            let rows = query.fetch_all(&mut *connection).await?;
            let streams = remove_replaced(&mut *connection, schema, &rows).await?;
            prune_stream_shapes(connection, schema, &streams).await?;
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
    Ok(None)
}

#[cfg(test)]
mod tests;
