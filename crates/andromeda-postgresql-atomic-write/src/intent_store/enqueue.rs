//! Enqueueing temporal append intents inside the entity anchor transaction.

use crate::intent_store::PostgresTemporalIntentStore;
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal_intent::TemporalIntent};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType, placeholder::external_placeholder, unnest::unnest_of};
use andromeda_postgresql_temporal_store::{error::TemporalStoreError, temporal_entry_record::TemporalIntentRecord};
use andromeda_temporal::error::TemporalError;
use sea_query::{Expr, Iden, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sqlx::{Postgres, Transaction};

/// The alias of one unnested intent, `input`.
#[derive(Iden, Clone, Copy)]
#[iden = "input"]
struct Input;

/// Insert every intent of one commit in one statement: the write identity
/// `$1` and its instant `$2` as scalars, the intents as one
/// `$3::temporal_intent[]`, each intent's entries travelling inside it.
///
/// One commit may land more than one entity, and every intent it produces
/// shares the commit's single write identity; that alone does not identify
/// one durable intent, because a batch commit whose side effects are
/// deferred (`SideEffectPlacement::DeferredOnLive`) writes an inline intent for
/// an entity's tombstones and later replays that same entity's deferred
/// side-effect job under the same write identity, producing a second intent
/// for the same entity whose entries occupy a different ordinal range. The
/// deduplicating key is therefore `(write_id, entity_id, base_ordinal)`, matching
/// `uq_temporal_append_intents_write_identity`, rather than `(write_id, entity_id)`.
/// `ON CONFLICT ... DO NOTHING` is what makes a commit replayed under its
/// original identity enqueue no second copy of any entity's intent at a given
/// base ordinal.
pub(super) fn enqueue_statement() -> Result<InsertStatement, SeaQueryError> {
    let source = Query::select()
        .expr(external_placeholder(1))
        .expr(external_placeholder(2))
        .expr(Expr::col((Input, TemporalIntent::EntityId)))
        .expr(Expr::col((Input, TemporalIntent::EntityTypeNameKeys)))
        .expr(Expr::col((Input, TemporalIntent::BaseOrdinal)))
        .expr(Expr::col((Input, TemporalIntent::Entries)))
        .from_function(unnest_of(cast(external_placeholder(3), PostgresType::TemporalIntentArray)), Input)
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
        let sql = enqueue_statement()
            .map_err(|error| TemporalError::from(PersistenceFailure::StatementUnbuildable.record(&error)))?
            .to_string(PostgresQueryBuilder);
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(record.write_id().as_uuid())
            .bind(record.recorded_at())
            .bind(intents)
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
    use sea_query::PostgresQueryBuilder;

    fn enqueue_sql() -> String {
        enqueue_statement().expect("arity").to_string(PostgresQueryBuilder)
    }

    #[test]
    fn every_intent_of_a_commit_is_one_row_of_one_statement() {
        let sql = enqueue_sql();
        assert!(sql.starts_with(r#"INSERT INTO "temporal_append_intents""#), "{sql}");
        assert!(sql.contains(r#"FROM UNNEST($3::temporal_intent[]) AS "input""#), "{sql}");
        assert_eq!(sql.matches("UNNEST").count(), 1, "{sql}");
    }

    /// Clause 4.5.6: the types travel on the intent row, so the drain can
    /// bring the Temporal Evolution into being; the write's instant travels
    /// beside them once, as every entry's `recorded_at`.
    #[test]
    fn an_intent_row_carries_its_types_and_recorded_instant() {
        let sql = enqueue_sql();
        assert!(
            sql.contains(r#"("write_id", "recorded_at", "entity_id", "entity_type_name_keys", "base_ordinal", "entries")"#),
            "{sql}"
        );
        assert!(sql.contains(r#"SELECT $1, $2, "input"."entity_id", "input"."entity_type_name_keys""#), "{sql}");
    }

    #[test]
    fn a_commit_replayed_under_its_original_identity_enqueues_no_second_intent_per_entity_and_base() {
        let sql = enqueue_sql();
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "entity_id", "base_ordinal") DO NOTHING"#), "{sql}");
    }

    /// No JSON leaves the broker for an intent: the entries are composites.
    #[test]
    fn no_intent_is_bound_as_json() {
        let sql = enqueue_sql();
        assert!(!sql.contains("json"), "{sql}");
        assert!(!sql.contains("payload"), "{sql}");
    }
}
