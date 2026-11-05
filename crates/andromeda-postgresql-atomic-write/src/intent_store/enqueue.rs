//! Enqueueing temporal append intents inside the entity anchor transaction.

use crate::intent_store::PostgresTemporalIntentStore;
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, temporal_intent::TemporalIntent};
use andromeda_postgresql_seaquery::{
    cast::cast,
    external_arguments::{ExternalArguments, ExternallyBound},
    pg_array::is_any_of,
    pg_type::PostgresType,
    unnest::unnest_of,
};
use andromeda_postgresql_temporal_store::{error::TemporalStoreError, temporal_entry_record::TemporalIntentRecord};
use andromeda_temporal::error::TemporalError;
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Expr, Func, Iden, OnConflict, Query, WithClause, WithQuery, error::Error as SeaQueryError};
use sqlx::{Postgres, Transaction};
use std::collections::BTreeSet;
use uuid::Uuid;

/// The alias of one unnested intent, `input`.
#[derive(Iden, Clone, Copy)]
#[iden = "input"]
struct Input;

/// The CTE raising the queued instant of every Entity the commit queues
/// history for, `raised`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "raised"]
struct Raised;

/// Insert every intent of one commit in one statement: the write identity
/// and its instant as scalars, the intents as one external
/// `temporal_intent[]` argument pushed onto `external`, each intent's entries
/// travelling inside it.
///
/// ```sql
/// WITH raised AS (UPDATE entities SET deleted_or_queued_at = GREATEST(entities.deleted_or_queued_at, $recorded_at)
///                 WHERE entities.id = ANY($entity_ids))
/// INSERT INTO temporal_append_intents (write_id, recorded_at, entity_id, …)
/// SELECT $write_id, $recorded_at, input.entity_id, … FROM UNNEST($intents::temporal_intent[]) AS input
/// ON CONFLICT (write_id, entity_id, base_ordinal) DO NOTHING
/// ```
///
/// One commit may land more than one entity, and every intent it produces
/// shares the commit's single write identity, so that alone does not identify
/// one durable intent: an intent is the first ordinal its entries occupy in
/// the write's one numbering. The deduplicating key is therefore `(write_id,
/// entity_id, base_ordinal)`, matching
/// `uq_temporal_append_intents_write_identity`. `ON CONFLICT ... DO NOTHING`
/// is what makes a commit replayed under its original identity enqueue no
/// second copy of any entity's intent at a given base ordinal.
///
/// A queued history reaches `evolution_modified_at` only when the drain
/// applies it, so the statement also raises each identity row's
/// `deleted_or_queued_at` to `recorded_at`: a write of the current state that
/// locks the row reads the instant from it, whichever snapshot its statement
/// took. The commit holds
/// every such row's lock already, so the update waits on nobody. An Entity
/// whose deletion removed its identity row has none to raise.
pub(super) fn enqueue_statement(
    external: &mut ExternalArguments<'_>,
    write_id: Uuid,
    recorded_at: DateTime<Utc>,
    intents: Vec<TemporalIntentRecord>,
) -> Result<WithQuery, SeaQueryError> {
    let entity_ids: BTreeSet<String> = intents.iter().map(|intent| intent.entity_id.clone()).collect(); // clone: the bound key list owns its identifiers while the intents move into their own argument
    let raised = Query::update()
        .table(Entities::Table)
        .value(
            Entities::DeletedOrQueuedAt,
            Func::greatest([Expr::col((Entities::Table, Entities::DeletedOrQueuedAt)), Expr::val(recorded_at)]),
        )
        .and_where(is_any_of(Expr::col((Entities::Table, Entities::Id)), entity_ids))
        .to_owned();
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
    Ok(WithClause::new()
        .cte(CommonTableExpression::new().table_name(Raised).query(raised).to_owned())
        .to_owned()
        .query(statement))
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::intent_store::enqueue::enqueue_statement;
    use andromeda_postgresql_seaquery::external_arguments::ExternalArguments;
    use andromeda_postgresql_temporal_store::temporal_entry_record::TemporalIntentRecord;
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value, Values};
    use uuid::Uuid;

    /// An intent of `entity_id` holding no entry.
    fn intent(entity_id: &str, base_ordinal: i32) -> TemporalIntentRecord {
        TemporalIntentRecord {
            entity_id: entity_id.to_owned(),
            entity_type_name_keys: Vec::new(),
            base_ordinal,
            entries: Vec::new(),
            entry_needs: Vec::new(),
        }
    }

    fn enqueue_sql_of(intents: Vec<TemporalIntentRecord>) -> (String, Values) {
        let mut external = ExternalArguments::default();
        enqueue_statement(&mut external, Uuid::nil(), Utc::now(), intents)
            .expect("arity")
            .build(PostgresQueryBuilder)
    }

    fn enqueue_sql() -> String {
        let (sql, values) = enqueue_sql_of(vec![intent("urn:ngsi-ld:Vehicle:1", 0)]);
        assert_eq!(
            values.0.len(),
            4,
            "the raised instant, the Entity, the write identity and its instant are the statement's own values: {sql}"
        );
        sql
    }

    /// A write of the current state reads the queued instant from the row it
    /// locks, never from a queue its statement's snapshot may predate.
    #[test]
    fn the_commit_raises_the_queued_instant_of_every_entity_it_queues_history_for() {
        let sql = enqueue_sql();
        assert!(
            sql.starts_with(concat!(
                r#"WITH "raised" AS (UPDATE "entities" SET "deleted_or_queued_at" = GREATEST("entities"."deleted_or_queued_at", $1) "#,
                r#"WHERE "entities"."id" = $2) INSERT INTO "temporal_append_intents""#
            )),
            "{sql}"
        );
    }

    /// Two intents of one Entity, at two base ordinals, raise its row once.
    #[test]
    fn an_entity_with_several_intents_is_named_once() {
        let (sql, values) = enqueue_sql_of(vec![
            intent("urn:ngsi-ld:Vehicle:2", 0),
            intent("urn:ngsi-ld:Vehicle:1", 1),
            intent("urn:ngsi-ld:Vehicle:2", 3),
        ]);
        assert!(sql.contains(r#"WHERE "entities"."id" = ANY($2)) INSERT"#), "{sql}");
        let named: Value = vec!["urn:ngsi-ld:Vehicle:1".to_owned(), "urn:ngsi-ld:Vehicle:2".to_owned()].into();
        assert_eq!(values.0.get(1), Some(&named), "{sql}");
    }

    #[test]
    fn every_intent_of_a_commit_is_one_row_of_one_statement() {
        let sql = enqueue_sql();
        assert!(sql.contains(r#") INSERT INTO "temporal_append_intents""#), "{sql}");
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
        assert!(sql.contains(r#"SELECT $3, $4, "input"."entity_id", "input"."entity_type_name_keys""#), "{sql}");
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
