//! Applying temporal append intents to a branch's history in SQL.
//!
//! An intent's entries are already in the shape of the history rows they
//! become (`temporal_entry`), so an application is a fixed sequence of
//! statements that read the intents where they are: the claimed rows of the
//! queue for the drain, the commit's own intents bound as one array for a
//! commit that records natively. The statements write the Temporal Evolutions
//! and streams, the instance replacements and claims, the
//! history rows, the stream extents covering their geometries and the stream
//! shapes their values carry, and the Scope rows; nothing is read back into
//! the broker beyond the keys of the evolutions the application created, which
//! the statements after them are sent, the types the existing evolutions
//! gained, whose streams the stream census counts next, and the rows a
//! replacement removes. The
//! drain runs the sequence in the transaction that also removes the intents, so a
//! reader sees each entry queued or recorded, never both.

/// The catalogue an application establishes first.
mod catalog_statements;
/// Which ends of an instance an application records.
mod deletion_judgement;
/// The Temporal Evolutions a commit's own create established.
mod established_evolutions;
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
        catalog_statements::{evolutions_and_streams_statement, untyped_observations_statement, untyped_streams_statement},
        history_statements::{
            claims_statement,
            extent_growth_statement,
            history_statement,
            released_claims_statement,
            replaced_instances_statement,
            scope_history_statement,
            stream_shapes_statement,
        },
        intent_source::{E, EntryReads, Input, IntentSource, StagedSource},
        replaced_rows::remove_replaced,
    },
    stored_catalog_error::StoredCatalogError,
    stream_census::{GainedType, gained_types_census},
    stream_shape_pruning::prune_stream_shapes,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
use andromeda_postgresql_seaquery::{
    external_arguments::{ExternalArguments, ExternallyBound},
    statement::Statement,
    unnest::UnnestAliased,
};
use andromeda_temporal::temporal_not_found_reason::TemporalNotFoundReason;
use derive_more::From;
use sea_query::{DeleteStatement, DynIden, InsertStatement, SelectStatement, UpdateStatement, WithQuery, error::Error as SeaQueryError};
use sqlx::{
    PgConnection,
    Postgres,
    Row,
    postgres::{PgArguments, PgRow},
    query::Query,
};

/// What the evolution step of an application wrote.
#[derive(Debug, Default)]
struct Evolutions {
    /// The keys of the Temporal Evolutions it created.
    created: Vec<i64>,
    /// The types the Temporal Evolutions that existed gained.
    gained: Vec<GainedType>,
}

impl Evolutions {
    /// The evolutions the rows of the evolution statement return, each an
    /// entity key, whether the statement created it and the types it gained.
    fn of(rows: &[PgRow]) -> Result<Evolutions, sqlx::Error> {
        let mut evolutions = Evolutions::default();
        for row in rows {
            let entity_key: i64 = row.try_get(0)?;
            if row.try_get(1)? {
                evolutions.created.push(entity_key);
            }
            let gained: Vec<i32> = row.try_get(2)?;
            evolutions.gained.extend(gained.into_iter().map(|type_key| GainedType { entity_key, type_key }));
        }
        Ok(evolutions)
    }
}

/// What the application does with one statement's outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepRole {
    /// Run it.
    Write,
    /// Run it, keep the keys of the Temporal Evolutions it returns as
    /// created and count the types it returns as gained into the stream
    /// census.
    Evolve,
    /// Run it and refuse the application when it returns a row.
    RefuseUntyped,
    /// Run it and refuse a duplicate `instanceId` claim as the client's own.
    Claim,
    /// Read the attribute history rows the entries replace, then remove
    /// exactly those rows and prune their streams' shapes.
    Replace,
}

/// A statement of the application, of whichever kind.
#[derive(Debug, From)]
enum StepStatement {
    /// A read.
    Select(SelectStatement),
    /// An insert.
    Insert(InsertStatement),
    /// A delete.
    Delete(DeleteStatement),
    /// An update.
    Update(UpdateStatement),
    /// A `WITH` query.
    With(WithQuery),
}

impl StepStatement {
    /// The driver query running the statement, its entries and bound intents
    /// unnested with their positions and `external` the arguments the staged
    /// source placed in it.
    fn into_query(self, external: ExternalArguments<'_>) -> Query<'static, Postgres, PgArguments> {
        match self {
            StepStatement::Select(statement) => ExternallyBound::new(positioned(statement), external).into_query(),
            StepStatement::Insert(statement) => ExternallyBound::new(positioned(statement), external).into_query(),
            StepStatement::Delete(statement) => ExternallyBound::new(positioned(statement), external).into_query(),
            StepStatement::Update(statement) => ExternallyBound::new(positioned(statement), external).into_query(),
            StepStatement::With(statement) => ExternallyBound::new(positioned(statement), external).into_query(),
        }
    }
}

/// `statement` with every entry unnest and the bound intents unnest numbering
/// its rows, which the composite arrays need `WITH ORDINALITY` for.
fn positioned<S>(statement: S) -> UnnestAliased<S> {
    UnnestAliased::new(statement).with_record_ordinality(E).with_record_ordinality(Input)
}

/// Builds the statement of one step over a staged source in the branch whose
/// schema is given.
type StepBuilder = fn(&StagedSource<'_>, Option<&DynIden>) -> Result<StepStatement, SeaQueryError>;

/// One statement of the application, before it is built.
struct Step {
    /// What its outcome means.
    role: StepRole,
    /// What the source has to hold for it to change anything; `None` when it
    /// runs for every source.
    need: Option<TemporalIntentReach>,
    /// What of the entries it reads.
    reads: EntryReads,
    /// The statement it runs.
    build: StepBuilder,
}

impl Step {
    /// `build` in `role`, reading `reads` of the entries and changing
    /// something only where the source meets `need`.
    const fn of(role: StepRole, need: Option<TemporalIntentReach>, reads: EntryReads, build: StepBuilder) -> Step {
        Step { role, need, reads, build }
    }
}

/// The statements applying the intents of a source to a branch, in the order
/// they run: the Temporal Evolutions and streams before the history that
/// references them, the refusal of an untyped observation after the
/// evolutions the source itself creates, the streams of the intents stating
/// no type after that refusal, the instance replacements before the claims
/// and rows that take their place, the stream extents and shapes after the
/// history rows whose geometries and values they cover. Every statement
/// reading attribute entries after the evolutions are written reads the keys
/// of the ones the application created, which decide the ends of an instance
/// it records.
fn application_steps() -> Vec<Step> {
    let (identities, everything) = (EntryReads::Identities, EntryReads::Everything);
    vec![
        Step::of(StepRole::Evolve, Some(TemporalIntentReach::TypedIntent), everything, |source, schema| {
            evolutions_and_streams_statement(source, schema).map(StepStatement::from)
        }),
        Step::of(
            StepRole::RefuseUntyped,
            Some(TemporalIntentReach::UntypedObservation),
            identities,
            |source, schema| Ok(untyped_observations_statement(source, schema).into()),
        ),
        Step::of(
            StepRole::Write,
            Some(TemporalIntentReach::UntypedAttributeEntry),
            everything,
            |source, schema| untyped_streams_statement(source, schema).map(StepStatement::from),
        ),
        Step::of(StepRole::Replace, Some(TemporalIntentReach::VerbatimInstance), identities, |source, schema| {
            Ok(replaced_instances_statement(source, schema).into())
        }),
        Step::of(StepRole::Write, Some(TemporalIntentReach::VerbatimInstance), identities, |source, schema| {
            Ok(released_claims_statement(source, schema).into())
        }),
        Step::of(StepRole::Claim, Some(TemporalIntentReach::VerbatimInstance), identities, |source, schema| {
            claims_statement(source, schema).map(StepStatement::from)
        }),
        Step::of(StepRole::Write, Some(TemporalIntentReach::AttributeEntry), everything, |source, schema| {
            history_statement(source, schema).map(StepStatement::from)
        }),
        Step::of(StepRole::Write, Some(TemporalIntentReach::AttributeEntry), everything, |source, schema| {
            Ok(extent_growth_statement(source, schema).into())
        }),
        Step::of(StepRole::Write, Some(TemporalIntentReach::AttributeEntry), everything, |source, schema| {
            Ok(stream_shapes_statement(source, schema).into())
        }),
        Step::of(StepRole::Write, Some(TemporalIntentReach::ScopeEntry), everything, |source, schema| {
            scope_history_statement(source, schema).map(StepStatement::from)
        }),
    ]
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
    for step in application_steps() {
        if step.need.is_none_or(|need| source.may_meet(need))
            && let Some(evolutions) = run(connection, source, schema.as_ref(), &created, step).await?
        {
            count_gained_types(connection, schema.as_ref(), &evolutions.gained).await?;
            created = evolutions.created;
        }
    }
    Ok(())
}

/// Count every stream of the evolutions in `gained` into the stream census of
/// the branch `schema` names, under the type each gained.
async fn count_gained_types(connection: &mut PgConnection, schema: Option<&DynIden>, gained: &[GainedType]) -> Result<(), TemporalStoreError> {
    if gained.is_empty() {
        return Ok(());
    }
    gained_types_census(schema, gained)
        .map_err(TemporalStoreError::StatementBuild)?
        .to_query()
        .execute(connection)
        .await?;
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
    let step = Step::of(
        StepRole::Write,
        Some(TemporalIntentReach::AttributeEntry),
        EntryReads::Everything,
        |source, schema| history_statement(source, schema).map(StepStatement::from),
    );
    run(connection, source, Some(schema), &[], step).await?;
    Ok(())
}

/// Run one step over `source` in the branch `schema` names, `created` the keys
/// of the evolutions the application created, and return what an
/// [`StepRole::Evolve`] step wrote.
async fn run(
    connection: &mut PgConnection,
    source: &IntentSource<'_>,
    schema: Option<&DynIden>,
    created: &[i64],
    Step { role, reads, build, .. }: Step,
) -> Result<Option<Evolutions>, TemporalStoreError> {
    let staged = StagedSource::new(source, reads, created);
    let statement = build(&staged, schema).map_err(TemporalStoreError::StatementBuild)?;
    let query = statement.into_query(staged.into_external());
    match role {
        StepRole::Write => {
            query.execute(&mut *connection).await?;
        }
        StepRole::Evolve => {
            let rows = query.fetch_all(&mut *connection).await?;
            return Ok(Some(Evolutions::of(&rows)?));
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
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
