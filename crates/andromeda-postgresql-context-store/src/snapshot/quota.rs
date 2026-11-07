//! Deterministic tenant-local Snapshot quota enforcement.

use crate::{
    snapshot::{
        clone_copy_session::end_copy_sessions,
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
    lock_wait_bound::LockWaitBound,
    maintenance::total_relation_size,
    operator::regex_match,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::{PgClass, PgNamespace},
};
use andromeda_snapshot::{error::SnapshotError, snapshot_invariant::SnapshotInvariant};
use sea_query::{Expr, ExprTrait, Func, JoinType, LockType, Query, SelectStatement};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;

/// How long eviction waits for a transaction holding the victim. Past it the
/// pass fails and the next one ranks again.
const VICTIM_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

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
        let victim = lowest_ranked_snapshot(&mut transaction)
            .await?
            .ok_or(SnapshotInvariant::QuotaWithoutEvictable)?;
        match lock_victim(&mut transaction, &victim).await? {
            VictimLock::Held => {}
            VictimLock::Rerank => continue,
        }
        reap_branch(&mut transaction, victim.snapshot_key).await?;
        delete_control_record(&mut transaction, victim.snapshot_key).await?;
        evicted.push(victim.id);
    }
    transaction.commit().await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(evicted)
}

/// The Snapshot the victim order ranks first.
#[derive(Debug)]
struct RankedVictim {
    /// The Snapshot's surrogate key.
    snapshot_key: SnapshotKey,
    /// The Snapshot's URI, as the eviction reports it.
    id: Identifier,
}

/// What eviction found once the lock on the Snapshot it ranked first was
/// granted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VictimLock {
    /// The Snapshot is still the one the order ranks first: it is evicted.
    Held,
    /// The Snapshot is gone, or the transaction the lock waited for changed
    /// the order: the pass ranks again, and holds the lock on a Snapshot it
    /// passed over until it commits.
    Rerank,
}

/// The Snapshot the victim order ranks first, read without taking its lock.
///
/// The choice must not depend on which rows are locked right now: skipping a
/// locked row would evict a better-ranked Snapshot than the order names.
async fn lowest_ranked_snapshot(transaction: &mut Transaction<'_, Postgres>) -> Result<Option<RankedVictim>, SnapshotError> {
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
    let Some((raw_key, raw_id)) = victim
        .limit(1)
        .to_query_as::<(i64, String)>()
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?
    else {
        return Ok(None);
    };
    Ok(Some(RankedVictim {
        snapshot_key: SnapshotKey::try_from(raw_key).map_err(|error| SnapshotError::from(undecodable(&error)))?,
        id: raw_id.parse().map_err(|error| SnapshotError::from(undecodable(&error)))?,
    }))
}

/// Take the lock eviction needs on the Snapshot the order ranked first.
///
/// An eviction is a deletion, so it first ends every clone copy into or out of
/// the Snapshot, which holds the control record for as long as the source is
/// large. The lock then waits up to [`VICTIM_LOCK_TIMEOUT`] for any other
/// transaction that holds it; the bound covers the lock alone, and the
/// transaction's own `lock_timeout` is put back before the branch is reaped.
///
/// A transaction the lock waited for may have changed the Snapshot's priority
/// or expiry, so the order is read again under the lock.
async fn lock_victim(transaction: &mut Transaction<'_, Postgres>, victim: &RankedVictim) -> Result<VictimLock, SnapshotError> {
    end_copy_sessions(transaction, &victim.id).await?;
    let bound = LockWaitBound::begin(transaction, VICTIM_LOCK_TIMEOUT)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let locked = lock_statement(victim.snapshot_key)
        .to_query()
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    bound.end(transaction).await.map_err(|error| persistence_error::<SnapshotError>(error))?;
    if locked.is_none() {
        return Ok(VictimLock::Rerank);
    }
    let first = lowest_ranked_snapshot(transaction).await?;
    Ok(victim_lock(victim, first.as_ref()))
}

/// Whether the locked `victim` is still the Snapshot the order ranks `first`.
fn victim_lock(victim: &RankedVictim, first: Option<&RankedVictim>) -> VictimLock {
    if first.is_some_and(|first| first.snapshot_key == victim.snapshot_key) {
        VictimLock::Held
    } else {
        VictimLock::Rerank
    }
}

/// `SELECT snapshot_key FROM snapshots WHERE snapshot_key = $1 FOR UPDATE`.
fn lock_statement(snapshot_key: SnapshotKey) -> SelectStatement {
    Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::SnapshotKey).eq(snapshot_key.as_i64()))
        .lock(LockType::Update)
        .to_owned()
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
    use crate::snapshot::{
        branch_schema::branch_tables,
        quota::{RankedVictim, VictimLock, branch_bytes_statement, lock_statement, victim_lock},
    };
    use andromeda_context_space::snapshot_key::{SnapshotKey, SnapshotKeyError};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn ranked(key: i64) -> RankedVictim {
        RankedVictim {
            snapshot_key: SnapshotKey::try_from(key).expect("a positive key"),
            id: format!("urn:ngsi-ld:Snapshot:{key}").parse().expect("a valid Snapshot URI"),
        }
    }

    #[test]
    fn a_locked_victim_the_order_still_ranks_first_is_evicted() {
        assert_eq!(victim_lock(&ranked(7), Some(&ranked(7))), VictimLock::Held);
    }

    /// The transaction the lock waited for raised the victim's priority or
    /// put its expiry back: the order names another Snapshot first, or none.
    #[test]
    fn a_locked_victim_the_order_no_longer_ranks_first_is_ranked_again() {
        assert_eq!(victim_lock(&ranked(7), Some(&ranked(8))), VictimLock::Rerank);
        assert_eq!(victim_lock(&ranked(7), None), VictimLock::Rerank);
    }

    /// Eviction names its victim by the order alone, then waits for exactly
    /// that row: a skip-locked read would pass over a locked lowest-ranked
    /// Snapshot and evict the next one.
    #[test]
    fn the_victim_is_locked_by_key_and_waited_for() -> Result<(), SnapshotKeyError> {
        let sql = lock_statement(SnapshotKey::try_from(7)?).to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT "snapshot_key" FROM "snapshots" WHERE "snapshot_key" = 7 FOR UPDATE"#);
        Ok(())
    }

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
