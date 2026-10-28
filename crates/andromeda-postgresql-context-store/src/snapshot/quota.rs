//! Deterministic tenant-local Snapshot quota enforcement.

use crate::{
    snapshot::{
        reap::{delete_control_record, reap_branch},
        record::victim_order,
    },
    store_fault::{persistence_error, undecodable},
};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::SNAPSHOT_SCHEMA_PATTERN;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::{
    advisory_lock::advisory_xact_lock,
    cast::cast,
    literal::text_literal,
    maintenance::total_relation_size,
    operator::regex_match,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::{PgClass, PgNamespace},
};
use andromeda_snapshot::{error::SnapshotError, snapshot_invariant::SnapshotInvariant};
use sea_query::{Expr, ExprTrait, Func, JoinType, LockBehavior, LockType, Query, SelectStatement};
use sqlx::{PgPool, Postgres, Row, Transaction};

/// Remove deterministic victims until both configured limits are satisfied.
pub(crate) async fn enforce_snapshot_quotas(pool: &PgPool, count_quota: Option<usize>, byte_quota: Option<u64>) -> Result<Vec<Identifier>, SnapshotError> {
    if count_quota.is_none() && byte_quota.is_none() {
        return Ok(Vec::new());
    }
    let count_quota = count_quota
        .map(i64::try_from)
        .transpose()
        .map_err(|error| SnapshotError::from(undecodable(&error)))?;
    let byte_quota = byte_quota
        .map(i64::try_from)
        .transpose()
        .map_err(|error| SnapshotError::from(undecodable(&error)))?;
    let mut transaction = pool.begin().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    Query::select()
        .expr(advisory_xact_lock(Expr::val("andromeda.snapshot.quota")))
        .to_owned()
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let mut evicted = Vec::new();
    loop {
        let count = snapshot_count(&mut transaction).await?;
        let count_within = count_quota.is_none_or(|maximum| count <= maximum);
        // The byte footprint is only measured when a byte quota bounds it: the
        // scan over the branch schemas must not run for a count-only quota, where
        // it would needlessly race a concurrent branch promotion or reap.
        let bytes_within = match byte_quota {
            Some(maximum) => branch_bytes(&mut transaction).await? <= maximum,
            None => true,
        };
        if count_within && bytes_within {
            break;
        }
        let mut victim = Query::select();
        victim
            .column((Snapshots::Table, Snapshots::SnapshotKey))
            .column((Snapshots::Table, Snapshots::Id))
            .from(Snapshots::Table)
            .join(
                JoinType::Join,
                SnapshotPreparation::Table,
                Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
            );
        victim_order(&mut victim);
        victim
            .limit(1)
            .lock_with_tables_behavior(LockType::Update, [Snapshots::Table], LockBehavior::SkipLocked);
        let row = victim
            .to_query()
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| persistence_error::<SnapshotError>(error))?
            .ok_or(SnapshotInvariant::QuotaWithoutEvictable)?;
        let raw_key: i64 = row.try_get("snapshot_key").map_err(|error| persistence_error::<SnapshotError>(error))?;
        let raw_id: String = row.try_get("id").map_err(|error| persistence_error::<SnapshotError>(error))?;
        let snapshot_key = SnapshotKey::try_from(raw_key).map_err(|error| SnapshotError::from(undecodable(&error)))?;
        reap_branch(&mut transaction, snapshot_key).await?;
        delete_control_record(&mut transaction, snapshot_key).await?;
        evicted.push(raw_id.parse().map_err(|error| SnapshotError::from(undecodable(&error)))?);
    }
    transaction.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(evicted)
}

/// The number of Snapshots the tenant currently holds.
async fn snapshot_count(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, SnapshotError> {
    Query::select()
        .expr(Func::count(Expr::col(Snapshots::SnapshotKey)))
        .from(Snapshots::Table)
        .to_owned()
        .to_query_scalar()
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))
}

/// The total on-disk size of every Snapshot branch schema.
///
/// Each Snapshot's captured state lives in its own branch schemas, published
/// and staging, named as the branch naming convention names them, so one sum
/// over those schemas' relations is the materialised byte footprint the quota
/// bounds.
async fn branch_bytes(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, SnapshotError> {
    branch_bytes_statement()
        .to_query_scalar()
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))
}

/// `SELECT COALESCE(SUM(pg_total_relation_size(c.oid)), 0)::bigint FROM
/// pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname ~
/// <branch schema pattern> AND c.relkind = 'r'`.
///
/// The relations are summed by catalogue OID rather than by re-resolving a
/// `schema.table` name: a Snapshot branch being promoted (its staging schema
/// renamed) or reaped concurrently would make a name lookup fail, whereas
/// `pg_total_relation_size` of an OID that has since vanished yields NULL,
/// which the sum ignores. Only ordinary tables carry storage here (`relkind =
/// 'r'`), and their indexes and TOAST are folded into each total; no relation
/// is named, so every table a branch migration creates counts.
fn branch_bytes_statement() -> SelectStatement {
    let total = Func::coalesce([
        Func::sum(total_relation_size(Expr::col((PgClass::Table, PgClass::Oid)))).into(),
        Expr::val(0_i64),
    ]);
    Query::select()
        .expr(cast(total.into(), PostgresType::Bigint))
        .from(PgClass::Table)
        .join(
            JoinType::Join,
            PgNamespace::Table,
            Expr::col((PgNamespace::Table, PgNamespace::Oid)).equals((PgClass::Table, PgClass::Relnamespace)),
        )
        .and_where(regex_match(
            Expr::col((PgNamespace::Table, PgNamespace::Nspname)),
            Expr::val(SNAPSHOT_SCHEMA_PATTERN),
        ))
        .and_where(Expr::col((PgClass::Table, PgClass::Relkind)).eq(text_literal("r")))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{branch_schema::branch_tables, quota::branch_bytes_statement};
    use sea_query::PostgresQueryBuilder;

    /// A Snapshot's footprint is its whole branch schema, so the instance
    /// table and its key index count without being listed, and reaping drops
    /// the schema with everything it holds.
    #[test]
    fn purge_and_quota_count_the_instance_table() {
        let sql = branch_bytes_statement().to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT COALESCE(SUM(pg_total_relation_size("pg_class"."oid")), 0)::bigint FROM "pg_class" JOIN "pg_namespace" ON "pg_namespace"."oid" = "pg_class"."relnamespace" WHERE ("pg_namespace"."nspname" ~ '^snap_([1-9][0-9]*)(_stg)?$') AND "pg_class"."relkind" = ('r')"#
        );
        assert!(branch_tables().iter().any(|table| table.to_string() == "current_attributes"));
    }
}
