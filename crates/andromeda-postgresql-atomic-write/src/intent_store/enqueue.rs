//! Enqueueing temporal append intents inside the entity anchor transaction.

use crate::intent_store::PostgresTemporalIntentStore;
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal_intent::TemporalIntent};
use andromeda_postgresql_seaquery::{
    cast::cast,
    external_arguments::{ExternalArguments, ExternallyBound},
    pg_type::PostgresType,
    unnest::unnest_of,
};
use andromeda_postgresql_temporal_store::{error::TemporalStoreError, temporal_entry_record::TemporalIntentRecord};
use andromeda_temporal::error::TemporalError;
use chrono::{DateTime, Utc};
use sea_query::{Expr, Iden, InsertStatement, OnConflict, Query, error::Error as SeaQueryError};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// The alias of one unnested intent, `input`.
#[derive(Iden, Clone, Copy)]
#[iden = "input"]
struct Input;

/// Insert every intent of one commit in one statement: the write identity
/// and its instant as scalars, the intents as one external
/// `temporal_intent[]` argument pushed onto `external`, each intent's entries
/// travelling inside it.
///
/// One commit may land more than one entity, and every intent it produces
/// shares the commit's single write identity, so that alone does not identify
/// one durable intent: an intent is the first ordinal its entries occupy in
/// the write's one numbering. The deduplicating key is therefore `(write_id,
/// entity_id, base_ordinal)`, matching
/// `uq_temporal_append_intents_write_identity`. `ON CONFLICT ... DO NOTHING`
/// is what makes a commit replayed under its original identity enqueue no
/// second copy of any entity's intent at a given base ordinal.
pub(super) fn enqueue_statement(
    external: &mut ExternalArguments<'_>,
    write_id: Uuid,
    recorded_at: DateTime<Utc>,
    intents: Vec<TemporalIntentRecord>,
) -> Result<InsertStatement, SeaQueryError> {
    let intents = external.push(intents);
    let source = Query::select()
        .expr(Expr::val(write_id))
        .expr(Expr::val(recorded_at))
        .expr(Expr::col((Input, TemporalIntent::EntityId)))
        .expr(Expr::col((Input, TemporalIntent::EntityTypeNameKeys)))
        .expr(Expr::col((Input, TemporalIntent::BaseOrdinal)))
        .expr(Expr::col((Input, TemporalIntent::Entries)))
        .expr(Expr::col((Input, TemporalIntent::EntryNeeds)))
        .from_function(unnest_of(cast(intents, PostgresType::TemporalIntentArray)), Input)
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(TemporalAppendIntents::Table)
        .columns([
            TemporalAppendIntents::WriteId,
            TemporalAppendIntents::RecordedAt,
            TemporalAppendIntents::EntityId,
            TemporalAppendIntents::EntityTypeNameKeys,
            TemporalAppendIntents::BaseOrdinal,
            TemporalAppendIntents::Entries,
            TemporalAppendIntents::EntryNeeds,
        ])
        .select_from(source)?
        .on_conflict(
            OnConflict::columns([
                TemporalAppendIntents::WriteId,
                TemporalAppendIntents::EntityId,
                TemporalAppendIntents::BaseOrdinal,
            ])
            .do_nothing()
            .to_owned(),
        );
    Ok(statement)
}

impl PostgresTemporalIntentStore {
    /// Persist the intents of the commit `record` inside the entity anchor
    /// transaction, in the live branch's queue: only the live branch defers
    /// its history.
    pub(crate) async fn enqueue_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        record: &WriteRecord,
        intents: Vec<TemporalIntentRecord>,
    ) -> Result<(), TemporalError> {
        if intents.is_empty() {
            return Ok(());
        }
        let mut external = ExternalArguments::default();
        let statement = enqueue_statement(&mut external, record.write_id().as_uuid(), record.recorded_at(), intents)
            .map_err(|error| TemporalError::from(PersistenceFailure::StatementUnbuildable.record(&error)))?;
        ExternallyBound::new(statement, external)
            .into_query()
            .execute(&mut **tx)
            .await
            // Classified like every other temporal-store statement, so a
            // saturated pool or a rolled-back transaction keeps its category.
            .map_err(|error| TemporalError::from(TemporalStoreError::from(error)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::intent_store::enqueue::enqueue_statement;
    use andromeda_postgresql_seaquery::external_arguments::ExternalArguments;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn enqueue_sql() -> String {
        let mut external = ExternalArguments::default();
        let (sql, values) = enqueue_statement(&mut external, Uuid::nil(), Utc::now(), Vec::new())
            .expect("arity")
            .build(PostgresQueryBuilder);
        assert_eq!(values.0.len(), 2, "the write identity and its instant are the statement's own values: {sql}");
        sql
    }

    #[test]
    fn every_intent_of_a_commit_is_one_row_of_one_statement() {
        let sql = enqueue_sql();
        assert!(sql.starts_with(r#"INSERT INTO "temporal_append_intents""#), "{sql}");
        assert!(sql.contains(r#"::temporal_intent[]) AS "input""#), "{sql}");
        assert_eq!(sql.matches("UNNEST").count(), 1, "{sql}");
    }

    /// Clause 4.5.6: the types travel on the intent row, so the drain can
    /// bring the Temporal Evolution into being; the write's instant travels
    /// beside them once, as every entry's `recorded_at`.
    #[test]
    fn an_intent_row_carries_its_types_and_recorded_instant() {
        let sql = enqueue_sql();
        assert!(
            sql.contains(r#"("write_id", "recorded_at", "entity_id", "entity_type_name_keys", "base_ordinal", "entries", "entry_needs")"#),
            "{sql}"
        );
        assert!(sql.contains(r#"SELECT $1, $2, "input"."entity_id", "input"."entity_type_name_keys""#), "{sql}");
    }

    #[test]
    fn a_commit_replayed_under_its_original_identity_enqueues_no_second_intent_per_entity_and_base() {
        let sql = enqueue_sql();
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "entity_id", "base_ordinal") DO NOTHING"#), "{sql}");
    }

    /// The drain decides which statements a batch needs from this column
    /// alone, so it comes from the intent the commit bound.
    #[test]
    fn an_intent_row_keeps_what_its_entries_reach() {
        let sql = enqueue_sql();
        assert!(sql.contains(r#""input"."entries", "input"."entry_needs" FROM"#), "{sql}");
    }

    /// No JSON leaves the broker for an intent: the entries are composites.
    #[test]
    fn no_intent_is_bound_as_json() {
        let sql = enqueue_sql();
        assert!(!sql.contains("json"), "{sql}");
        assert!(!sql.contains("payload"), "{sql}");
    }
}
