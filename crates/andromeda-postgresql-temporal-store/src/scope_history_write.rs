//! Bulk inserts into the `scope_history` hypertable.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    scope_history_entry::ScopeHistoryEntry,
    stored_instance_columns::{generated_instance_id, verbatim_instance_id},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::scope_value_kind::ScopeValueKind;
use andromeda_postgresql_query::ltree::StoredScopePath;
use andromeda_postgresql_schema::temporal::{ScopeHistory, TemporalEntities};
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, JoinType, OnConflict, PostgresQueryBuilder, Query, SimpleExpr};
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

/// The `UNNEST` staging alias, `d`, each set-write reads its parallel arrays under.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The `temporal_entities` alias the narrow key is resolved through.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
struct Te;

/// The staging columns of a scope-history row, named as the `AS d(…)` alias
/// spells them rather than as the `scope_history` columns they land in: the
/// entity id resolves the narrow key, and the label path is cast to `ltree`.
#[derive(Iden, Clone, Copy)]
enum Staged {
    /// Write clock of the row.
    RecordedAt,
    /// Entity identifier the temporal-entity key is resolved from.
    EntityId,
    /// Atomic write the row belongs to.
    WriteId,
    /// Position of the row within its atomic write.
    WriteOrdinal,
    /// Stored value shape of the row.
    ValueKind,
    /// Client-declared observation instant.
    ObservedAt,
    /// Instant the row was first recorded at.
    CreatedAt,
    /// Instant the row was last altered at.
    ModifiedAt,
    /// Deletion instant, present only on a tombstone.
    DeletedAt,
    /// Broker-minted instance identifier.
    GeneratedInstanceId,
    /// Instance identifier stored exactly as received.
    VerbatimInstanceId,
    /// Scope path in `ltree` label notation, absent on a tombstone.
    ScopePath,
    /// Scope as it was written, absent on a tombstone.
    VerbatimScopePath,
}

impl TemporalRepository {
    /// Bulk-insert scope-history observations in the caller's transaction.
    ///
    /// The narrow temporal-entity key each row is stored under is resolved by
    /// the statement itself, so a caller holding only the entity identifier does
    /// not read the catalogue first. A scope observation for an entity with no
    /// temporal catalogue entry is therefore dropped by the join rather than
    /// inserted against an invented key.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when a row's position within
    /// its write exceeds the stored column's width, and
    /// [`TemporalStoreError::Database`] when a statement fails.
    pub async fn insert_scope_history_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        entries: &[ScopeHistoryEntry],
    ) -> Result<(), TemporalStoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let binds = scope_history_binds(entries)?;
        let dedup_sql = scope_history_dedup_sql(target);
        sqlx::query(AssertSqlSafe(dedup_sql))
            .bind(&binds.entity_ids)
            .bind(&binds.generated_instance_ids)
            .bind(&binds.verbatim_instance_ids)
            .execute(&mut **tx)
            .await?;
        let insert_sql = scope_history_insert_sql(target)?;
        sqlx::query(AssertSqlSafe(insert_sql))
            .bind(&binds.recorded_ats)
            .bind(&binds.entity_ids)
            .bind(&binds.write_ids)
            .bind(&binds.write_ordinals)
            .bind(&binds.value_kinds)
            .bind(&binds.observed_ats)
            .bind(&binds.created_ats)
            .bind(&binds.modified_ats)
            .bind(&binds.deleted_ats)
            .bind(&binds.generated_instance_ids)
            .bind(&binds.verbatim_instance_ids)
            .bind(&binds.scope_paths)
            .bind(&binds.verbatim_scope_path)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}

/// Column-major binds for one scope-history batch.
struct ScopeHistoryBinds<'a> {
    /// Write clock of each row.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Entity identifier the temporal-entity key is resolved from.
    entity_ids: Vec<&'a str>,
    /// Atomic write each row belongs to.
    write_ids: Vec<Uuid>,
    /// Position of each row within its atomic write.
    write_ordinals: Vec<i32>,
    /// Stored value shape of each row.
    value_kinds: Vec<i16>,
    /// Client-declared observation instants.
    observed_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the rows were first recorded at.
    created_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the rows were last altered at.
    modified_ats: Vec<Option<DateTime<Utc>>>,
    /// Deletion instants, present only on tombstones.
    deleted_ats: Vec<Option<DateTime<Utc>>>,
    /// Broker-minted instance identifiers.
    generated_instance_ids: Vec<Option<Uuid>>,
    /// Instance identifiers stored exactly as received.
    verbatim_instance_ids: Vec<Option<&'a str>>,
    /// Scope paths in `ltree` label notation, absent on tombstones.
    scope_paths: Vec<Option<String>>,
    /// Scopes as they were written, absent on tombstones.
    verbatim_scope_path: Vec<Option<&'a str>>,
}

/// Spread one scope-history batch into its column-major binds.
fn scope_history_binds(entries: &[ScopeHistoryEntry]) -> Result<ScopeHistoryBinds<'_>, TemporalStoreError> {
    let mut binds = ScopeHistoryBinds {
        recorded_ats: Vec::with_capacity(entries.len()),
        entity_ids: Vec::with_capacity(entries.len()),
        write_ids: Vec::with_capacity(entries.len()),
        write_ordinals: Vec::with_capacity(entries.len()),
        value_kinds: Vec::with_capacity(entries.len()),
        observed_ats: Vec::with_capacity(entries.len()),
        created_ats: Vec::with_capacity(entries.len()),
        modified_ats: Vec::with_capacity(entries.len()),
        deleted_ats: Vec::with_capacity(entries.len()),
        generated_instance_ids: Vec::with_capacity(entries.len()),
        verbatim_instance_ids: Vec::with_capacity(entries.len()),
        scope_paths: Vec::with_capacity(entries.len()),
        verbatim_scope_path: Vec::with_capacity(entries.len()),
    };
    for entry in entries {
        binds.recorded_ats.push(entry.recorded_at);
        binds.entity_ids.push(entry.entity_id.as_str());
        binds.write_ids.push(entry.identity.write_id.as_uuid());
        binds
            .write_ordinals
            .push(entry.identity.ordinal.as_i32().map_err(TemporalStoreError::WriteOrdinal)?);
        binds.value_kinds.push(ScopeValueKind::code(entry.value.kind()));
        binds.observed_ats.push(entry.axes.observed_at);
        binds.created_ats.push(entry.axes.created_at);
        binds.modified_ats.push(entry.axes.modified_at);
        binds.deleted_ats.push(entry.value.deleted_at());
        binds.generated_instance_ids.push(generated_instance_id(&entry.instance));
        binds.verbatim_instance_ids.push(verbatim_instance_id(&entry.instance));
        let stored = entry.value.path().map(StoredScopePath::from);
        binds.verbatim_scope_path.push(stored.as_ref().map(|stored| stored.written));
        binds.scope_paths.push(stored.map(|stored| stored.label_path));
    }
    Ok(binds)
}

/// The dedup delete: a replayed batch removes the rows it is about to re-insert
/// so the insert's conflict clause has a clean slate.
///
/// `DELETE … USING` is expressed as a correlated `EXISTS`, since sea-query has
/// no `USING`. The three staging arrays carry NULLs and bind through `sqlx`; the
/// `datasetId`-style `IS NOT DISTINCT FROM` matches a NULL instance identifier
/// to a NULL one.
fn scope_history_dedup_sql(target: &ContextTarget) -> String {
    let columns = [
        UnnestColumn::new(Staged::EntityId, PostgresType::TextArray),
        UnnestColumn::new(Staged::GeneratedInstanceId, PostgresType::UuidArray),
        UnnestColumn::new(Staged::VerbatimInstanceId, PostgresType::TextArray),
    ];
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut matches = Query::select();
    matches
        .expr(Expr::val(1))
        .from_function(call, D)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalEntities::Table),
            Te,
            Expr::col((Te, TemporalEntities::Id)).equals((D, Staged::EntityId)),
        )
        .and_where(Expr::col((ScopeHistory::Table, ScopeHistory::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)))
        .and_where(
            SimpleExpr::from(Expr::col((ScopeHistory::Table, ScopeHistory::GeneratedInstanceId)))
                .not_distinct_from(Expr::col((D, Staged::GeneratedInstanceId))),
        )
        .and_where(
            SimpleExpr::from(Expr::col((ScopeHistory::Table, ScopeHistory::VerbatimInstanceId))).not_distinct_from(Expr::col((D, Staged::VerbatimInstanceId))),
        );
    let matches = matches;

    let sql = Query::delete()
        .from_table(table_of(target, ScopeHistory::Table))
        .and_where(Expr::exists(matches))
        .to_owned()
        .to_string(PostgresQueryBuilder);
    apply_column_alias(sql, &D, &columns)
}

/// The bulk scope-history insert, its narrow key resolved by a join so a caller
/// holding only the entity identifier does not read the catalogue first.
///
/// The thirteen staging arrays carry NULLs and bind through `sqlx`. The conflict
/// target names the columns `uq_scope_history_write_identity` covers, so a row replayed
/// under its original write identity is a no-op rather than a duplicate.
fn scope_history_insert_sql(target: &ContextTarget) -> Result<String, TemporalStoreError> {
    let columns = [
        UnnestColumn::new(Staged::RecordedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(Staged::EntityId, PostgresType::TextArray),
        UnnestColumn::new(Staged::WriteId, PostgresType::UuidArray),
        UnnestColumn::new(Staged::WriteOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(Staged::ValueKind, PostgresType::SmallintArray),
        UnnestColumn::new(Staged::ObservedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(Staged::CreatedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(Staged::ModifiedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(Staged::DeletedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(Staged::GeneratedInstanceId, PostgresType::UuidArray),
        UnnestColumn::new(Staged::VerbatimInstanceId, PostgresType::TextArray),
        UnnestColumn::new(Staged::ScopePath, PostgresType::TextArray),
        UnnestColumn::new(Staged::VerbatimScopePath, PostgresType::TextArray),
    ];
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let mut select = Query::select();
    select
        .expr(Expr::col((D, Staged::RecordedAt)))
        .expr(Expr::col((Te, TemporalEntities::TemporalEntityKey)))
        .expr(Expr::col((D, Staged::WriteId)))
        .expr(Expr::col((D, Staged::WriteOrdinal)))
        .expr(Expr::col((D, Staged::ValueKind)))
        .expr(Expr::col((D, Staged::ObservedAt)))
        .expr(Expr::col((D, Staged::CreatedAt)))
        .expr(Expr::col((D, Staged::ModifiedAt)))
        .expr(Expr::col((D, Staged::DeletedAt)))
        .expr(Expr::col((D, Staged::GeneratedInstanceId)))
        .expr(Expr::col((D, Staged::VerbatimInstanceId)))
        .expr(cast(Expr::col((D, Staged::ScopePath)), PostgresType::Ltree))
        .expr(Expr::col((D, Staged::VerbatimScopePath)))
        .from_function(call, D)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalEntities::Table),
            Te,
            Expr::col((Te, TemporalEntities::Id)).equals((D, Staged::EntityId)),
        );
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table_of(target, ScopeHistory::Table)).columns([
        ScopeHistory::RecordedAt,
        ScopeHistory::TemporalEntityKey,
        ScopeHistory::WriteId,
        ScopeHistory::WriteOrdinal,
        ScopeHistory::ValueKind,
        ScopeHistory::ObservedAt,
        ScopeHistory::CreatedAt,
        ScopeHistory::ModifiedAt,
        ScopeHistory::DeletedAt,
        ScopeHistory::GeneratedInstanceId,
        ScopeHistory::VerbatimInstanceId,
        ScopeHistory::ScopePath,
        ScopeHistory::VerbatimScopePath,
    ]);
    insert.select_from(select).map_err(TemporalStoreError::StatementBuild)?.on_conflict(
        OnConflict::columns([ScopeHistory::RecordedAt, ScopeHistory::WriteId, ScopeHistory::WriteOrdinal])
            .do_nothing()
            .to_owned(),
    );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

#[cfg(test)]
mod tests {
    use crate::scope_history_write::{scope_history_dedup_sql, scope_history_insert_sql};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_scope_row_hangs_off_the_temporal_catalogue_and_never_off_current_state() {
        let insert = scope_history_insert_sql(&ContextTarget::Live).expect("the fixed scope-history insert builds");
        for statement in [scope_history_dedup_sql(&ContextTarget::Live), insert] {
            assert!(
                statement.contains(r#"INNER JOIN "temporal_entities" AS "te" ON "te"."id" = "d"."entity_id""#),
                "{statement}"
            );
            assert!(!statement.contains(r#"FROM "entities""#), "{statement}");
        }
    }

    #[test]
    fn the_insert_carries_the_partition_column_and_the_whole_write_identity() {
        let insert = scope_history_insert_sql(&ContextTarget::Live).expect("the fixed scope-history insert builds");
        assert!(
            insert.contains(r#""recorded_at", "temporal_entity_key", "write_id", "write_ordinal""#),
            "{insert}"
        );
        assert!(insert.contains(r#""d"."scope_path"::ltree"#), "{insert}");
    }

    /// Clause 4.6.4: the Scope is read back as written, so the insert stores
    /// the written text beside the label path it is matched by.
    #[test]
    fn the_insert_stores_the_written_scope_beside_its_label_path() {
        let insert = scope_history_insert_sql(&ContextTarget::Live).expect("the fixed scope-history insert builds");
        assert!(insert.contains(r#""scope_path", "verbatim_scope_path")"#), "{insert}");
        assert!(insert.contains(r#""d"."verbatim_scope_path""#), "{insert}");
        assert!(insert.contains("$13::text[]"), "{insert}");
    }

    /// A commit replayed under its original write identity re-inserts every row
    /// it already wrote; the row-level conflict clause is what makes that second
    /// insert a no-op instead of a duplicate or an error.
    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        let insert = scope_history_insert_sql(&ContextTarget::Live).expect("the fixed scope-history insert builds");
        assert!(
            insert.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#),
            "{insert}"
        );
    }

    #[test]
    fn the_dedup_matches_a_null_instance_identity_to_a_null_one() {
        let sql = scope_history_dedup_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"DELETE FROM "scope_history" WHERE EXISTS"#), "{sql}");
        assert_eq!(sql.matches("IS NOT DISTINCT FROM").count(), 2, "{sql}");
        assert!(sql.contains("AS d(entity_id, generated_instance_id, verbatim_instance_id)"), "{sql}");
    }
}
