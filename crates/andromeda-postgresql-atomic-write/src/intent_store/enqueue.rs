//! Enqueueing temporal append intents inside the entity anchor transaction.

use crate::intent_store::PostgresTemporalIntentStore;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_array::text_array,
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_postgresql_temporal_store::error::TemporalStoreError;
use andromeda_temporal::{append_intent::TemporalAppendIntent, error::TemporalError};
use sea_query::{Alias, Expr, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// The `AS input(write_id, entity_id, base_ordinal, payload)` column-list alias the
/// enqueue's `UNNEST` set-write projects, in the order the parallel arrays are
/// bound and the insert's columns are named.
///
/// The payloads are bound as the intents' JSON text and cast to `jsonb` by the
/// projection, so no JSON tree is built for them on the way to the column.
pub(super) fn intent_unnest_columns() -> [UnnestColumn; 4] {
    [
        UnnestColumn::new(Alias::new("write_id"), PostgresType::UuidArray),
        UnnestColumn::new(Alias::new("entity_id"), PostgresType::TextArray),
        UnnestColumn::new(Alias::new("base_ordinal"), PostgresType::IntegerArray),
        UnnestColumn::new(Alias::new("payload"), PostgresType::TextArray),
    ]
}

/// Insert a batch of temporal append intents from four parallel arrays.
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
pub(super) fn enqueue_statement(ids: Vec<Uuid>, entity_ids: &[&str], bases: Vec<i32>, payloads: Vec<String>) -> Result<InsertStatement, SeaQueryError> {
    let columns = intent_unnest_columns();
    let call = unnest_call([
        (&columns[0], Expr::val(ids)),
        (&columns[1], text_array(entity_ids)),
        (&columns[2], Expr::val(bases)),
        (&columns[3], Expr::val(payloads)),
    ]);
    let input = Alias::new("input");
    let source = Query::select()
        .expr(Expr::col((input.clone(), Alias::new("write_id"))))
        .expr(Expr::col((input.clone(), Alias::new("entity_id"))))
        .expr(Expr::col((input.clone(), Alias::new("base_ordinal"))))
        .expr(cast(Expr::col((input.clone(), Alias::new("payload"))), PostgresType::Jsonb))
        .from_function(call, input)
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(TemporalAppendIntents::Table)
        .columns([
            TemporalAppendIntents::WriteId,
            TemporalAppendIntents::EntityId,
            TemporalAppendIntents::BaseOrdinal,
            TemporalAppendIntents::Payload,
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
    Ok(statement.clone())
}

impl PostgresTemporalIntentStore {
    /// Persist complete intents inside the entity anchor transaction, in the
    /// live branch's queue: only the live branch defers its history.
    pub(crate) async fn enqueue_in_tx(tx: &mut Transaction<'_, Postgres>, intents: &[TemporalAppendIntent]) -> Result<(), TemporalError> {
        if intents.is_empty() {
            return Ok(());
        }
        let mut ids = Vec::with_capacity(intents.len());
        let mut entity_ids = Vec::with_capacity(intents.len());
        let mut bases = Vec::with_capacity(intents.len());
        let mut payloads = Vec::with_capacity(intents.len());
        for intent in intents {
            ids.push(intent.record.write_id().as_uuid());
            entity_ids.push(intent.entity_id.as_str());
            bases.push(
                intent
                    .base_ordinal
                    .as_i32()
                    .map_err(|error| TemporalError::from(PersistenceFailure::Unencodable.record(&error)))?,
            );
            payloads.push(serde_json::to_string(intent).map_err(|error| TemporalError::from(PersistenceFailure::Unencodable.record(&error)))?);
        }
        let statement = enqueue_statement(ids, &entity_ids, bases, payloads)
            .map_err(|error| TemporalError::from(PersistenceFailure::StatementUnbuildable.record(&error)))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let sql = apply_column_alias(sql, &Alias::new("input"), &intent_unnest_columns());
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
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
    use crate::intent_store::enqueue::{enqueue_statement, intent_unnest_columns};
    use andromeda_postgresql_seaquery::unnest::apply_column_alias;
    use sea_query::{Alias, PostgresQueryBuilder};
    use uuid::Uuid;

    fn enqueue_sql() -> String {
        let statement = enqueue_statement(vec![Uuid::nil()], &["urn:ngsi-ld:Vehicle:1"], vec![0], vec!["{}".to_owned()]).expect("arity");
        let sql = statement.to_string(PostgresQueryBuilder);
        apply_column_alias(sql, &Alias::new("input"), &intent_unnest_columns())
    }
    #[test]
    fn a_commit_replayed_under_its_original_identity_enqueues_no_second_intent_per_entity_and_base() {
        let sql = enqueue_sql();
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "entity_id", "base_ordinal") DO NOTHING"#), "{sql}");
    }

    #[test]
    fn the_deduplicating_key_names_the_write_the_entity_and_the_ordinal_base() {
        let sql = enqueue_sql();
        assert!(
            sql.contains(r#"INTO "temporal_append_intents" ("write_id", "entity_id", "base_ordinal", "payload")"#),
            "{sql}"
        );
    }

    /// The parallel-array set-write projects a `AS input(...)` column-list
    /// alias with no ordinality, and each array is cast to its element type;
    /// the payloads travel as JSON text and become `jsonb` in the projection.
    #[test]
    fn the_enqueue_projects_the_four_parallel_arrays_as_named_columns() {
        let sql = enqueue_sql();
        assert!(sql.contains("AS input(write_id, entity_id, base_ordinal, payload)"), "{sql}");
        assert!(sql.contains("::uuid[]") && sql.contains("::text[]"), "{sql}");
        assert!(sql.contains(r#""input"."payload"::jsonb"#), "{sql}");
        assert!(!sql.contains("WITH ORDINALITY"), "{sql}");
    }

    /// A batch commit whose side effects are deferred keys on `base_ordinal` as
    /// well as `(write_id, entity_id)` so two intents for the same write and
    /// entity at different ordinals are not collided.
    #[test]
    fn two_intents_for_the_same_write_and_entity_are_distinguished_by_their_ordinal_base() {
        let sql = enqueue_sql();
        assert!(!sql.contains(r#"ON CONFLICT ("write_id", "entity_id") DO NOTHING"#), "{sql}");
    }
}
