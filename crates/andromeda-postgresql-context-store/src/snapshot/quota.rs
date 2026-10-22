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
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::advisory_lock::advisory_xact_lock;
use andromeda_snapshot::{error::SnapshotError, snapshot_invariant::SnapshotInvariant};
use sea_query::{Expr, ExprTrait, Func, JoinType, LockBehavior, LockType, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
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
    let lock = Query::select().expr(advisory_xact_lock(Expr::val("andromeda.snapshot.quota"))).to_owned();
    let (sql, values) = lock.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&mut *transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let mut evicted = Vec::new();
    loop {
        let count = snapshot_count(&mut transaction).await?;
        let count_within = count_quota.is_none_or(|maximum| count <= maximum);
        // The byte footprint is only measured when a byte quota bounds it: the
        // scan over `snap_%` relations must not run for a count-only quota, where
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
        let (sql, values) = victim.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
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
    let statement = Query::select()
        .expr(Func::count(Expr::col(Snapshots::SnapshotKey)))
        .from(Snapshots::Table)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))
}

/// The total on-disk size of every Snapshot branch schema.
///
/// Each Snapshot's captured state lives in a `snap_%` schema, so one sum over
/// those schemas' relations is the materialised byte footprint the quota bounds. The
/// underscore in the `LIKE` pattern is escaped so it matches literally.
///
/// The relations are summed by catalogue OID rather than by re-resolving a
/// `schema.table` name string: a Snapshot branch being promoted (its staging
/// schema renamed) or reaped concurrently would make a name lookup fail, whereas
/// `pg_total_relation_size` of an OID that has since vanished yields NULL, which
/// the sum ignores. Only ordinary tables carry storage here (`relkind = 'r'`);
/// their indexes and TOAST are already folded into the total.
async fn branch_bytes(transaction: &mut Transaction<'_, Postgres>) -> Result<i64, SnapshotError> {
    let bytes: i64 = sqlx::query_scalar(BRANCH_BYTES)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(bytes)
}

/// The statement [`branch_bytes`] runs. It names no relation, so every table a
/// branch migration creates counts with its indexes and TOAST.
const BRANCH_BYTES: &str = "SELECT COALESCE(SUM(pg_total_relation_size(c.oid)), 0)::bigint \
                            FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                            WHERE n.nspname LIKE 'snap\\_%' AND c.relkind = 'r'";

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::declared_schema::DeclaredSchema,
        snapshot::{branch_schema::BRANCH_TABLES, quota::BRANCH_BYTES},
    };

    /// A Snapshot's footprint is its whole branch schema, so the instance
    /// table and its key index count without being listed, and reaping drops
    /// the schema with everything it holds.
    #[test]
    fn purge_and_quota_count_the_instance_table() {
        assert!(
            BRANCH_BYTES.contains("pg_total_relation_size(c.oid)"),
            "indexes and TOAST included: {BRANCH_BYTES}"
        );
        assert!(BRANCH_BYTES.contains("n.nspname LIKE 'snap\\_%'"), "{BRANCH_BYTES}");
        assert!(BRANCH_BYTES.contains("c.relkind = 'r'"), "{BRANCH_BYTES}");
        assert!(!BRANCH_BYTES.contains("relname "), "no relation is singled out: {BRANCH_BYTES}");
        assert!(BRANCH_TABLES.contains(&"current_attributes"));
        let schema = DeclaredSchema::of_every_migration();
        assert!(
            schema
                .indexes
                .iter()
                .any(|index| index.name == "idx_ca_sub_eq_keys" && index.table == "current_attributes"),
            "the key index lives with its table in the branch schema"
        );
        assert!(
            schema.tables.iter().any(|table| table == "current_attributes"),
            "an ordinary table, which relkind 'r' selects"
        );
    }
}
