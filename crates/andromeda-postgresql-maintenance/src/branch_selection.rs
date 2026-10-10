//! Which Snapshot branches a tenant visit works in, and the lock that keeps
//! each one in place meanwhile.
//!
//! A visit works in a Snapshot branch only when the Snapshot is published and
//! outlives the maintenance statement timeout, so no statement of the visit
//! outlives the branch. Of those branches it works in the ones whose tables saw
//! updates or deletes, the most churned first: a branch only ever inserted into
//! (a capture or a clone nobody wrote to since) has nothing to reclaim.
//!
//! Each selected branch is locked with its Snapshot's branch lock, a session
//! lock held until the visit's tenant connection closes. A reap that finds the
//! lock held defers the schema drop instead of waiting for it. A reap may also
//! have committed between the eligibility read and the lock, so the control
//! record is read again once the lock is won.

use crate::{
    branch::Branch,
    capability::{self, MutationPrivilege},
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    scalar_query::select_bool,
    state::RegisteredTenant,
};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_postgresql_branch::{PUBLISHED_SNAPSHOT_SCHEMA_PATTERN, published_snapshot_key};
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
use andromeda_postgresql_context_store::snapshot::branch_lock::branch_lock_key;
use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::{
    advisory_lock::{advisory_unlock, try_advisory_lock},
    cast::cast,
    pg_type::PostgresType,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, JoinType, PostgresQueryBuilder, Query, SelectStatement, extension::postgres::PgBinOper};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};
use std::cmp::Reverse;
use strum::Display;
use tracing::{info, warn};

/// The shortest remaining lifetime of a Snapshot whose branch a visit works
/// in: the maintenance session's `statement_timeout`, so no rebuild or vacuum
/// of the visit runs past the Snapshot's expiry.
const EXPIRY_MARGIN: ChronoDuration = ChronoDuration::minutes(30);

/// Whether a visit may work in a Snapshot's branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BranchEligibility {
    /// Published and outliving [`EXPIRY_MARGIN`].
    Eligible,
    /// Still preparing in its private staging schema.
    Preparing,
    /// Expiring within [`EXPIRY_MARGIN`].
    Expiring,
}

/// One Snapshot's control record, as eligibility reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SnapshotRecord {
    /// The Snapshot.
    key: SnapshotKey,
    /// Its preparation status.
    status: SnapshotStatusCode,
    /// When the expiry reaper deletes it.
    expires_at: DateTime<Utc>,
}

impl SnapshotRecord {
    /// Whether a visit at `now` may work in the Snapshot's branch.
    fn eligibility(self, now: DateTime<Utc>) -> BranchEligibility {
        match self.status {
            SnapshotStatusCode::Preparing => BranchEligibility::Preparing,
            SnapshotStatusCode::Success | SnapshotStatusCode::Partial | SnapshotStatusCode::Empty | SnapshotStatusCode::Failure => {
                if self.expires_at.signed_duration_since(now) > EXPIRY_MARGIN {
                    BranchEligibility::Eligible
                } else {
                    BranchEligibility::Expiring
                }
            }
        }
    }
}

/// The rows the tables of one published Snapshot branch updated or deleted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BranchChurn {
    /// The Snapshot.
    key: SnapshotKey,
    /// Updated and deleted rows since the statistics were reset.
    tuple_changes: u64,
}

/// Why a visit skips a branch whose lock it won.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
enum BranchDecline {
    /// The Snapshot was reaped after the eligibility read.
    SnapshotReaped,
    /// The Snapshot became ineligible after the eligibility read.
    SnapshotIneligible,
    /// Automatic mode lacks `MAINTAIN` on a heap of the branch.
    MissingMaintain,
}

/// Lock every eligible Snapshot branch with churn, the most churned first,
/// and return the ones the visit works in. Each returned branch stays locked
/// until the tenant connection closes; a branch another session holds is
/// skipped.
pub(crate) async fn lock_branches(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    mutation: MutationPrivilege,
) -> Result<Vec<SnapshotKey>, MaintenanceError> {
    let now = Utc::now();
    let eligible = snapshot_records(connection, None)
        .await?
        .into_iter()
        .filter(|record| record.eligibility(now) == BranchEligibility::Eligible)
        .map(|record| record.key)
        .collect::<Vec<_>>();
    let churn = branch_churn(connection).await?;
    let mut locked = Vec::new();
    for key in churn_order(&eligible, &churn) {
        if lock_branch(connection, tenant, key, mutation).await? {
            locked.push(key);
        }
    }
    Ok(locked)
}

/// The eligible branches with churn, the most churned first, ties by key. A
/// branch without churn is left out.
fn churn_order(eligible: &[SnapshotKey], churn: &[BranchChurn]) -> Vec<SnapshotKey> {
    let mut ordered = churn
        .iter()
        .filter(|branch| branch.tuple_changes > 0 && eligible.contains(&branch.key))
        .map(|branch| (Reverse(branch.tuple_changes), branch.key))
        .collect::<Vec<_>>();
    ordered.sort_unstable();
    ordered.into_iter().map(|(_, key)| key).collect()
}

/// Take one Snapshot's branch lock without waiting, then check again that the
/// visit may work in the branch. Returns whether it holds the lock; a branch
/// it declines is unlocked again.
async fn lock_branch(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    key: SnapshotKey,
    mutation: MutationPrivilege,
) -> Result<bool, MaintenanceError> {
    if !select_bool(connection, Query::select().expr(try_advisory_lock(branch_lock_key(key))).to_owned()).await? {
        info!(tenant = tenant.log_name(), database = %tenant.database, snapshot_key = key.as_i64(), decision = "deferred", reason = "branch-locked", "PostgreSQL maintenance skipped a Snapshot branch another session holds");
        return Ok(false);
    }
    let Some(decline) = decline(connection, key, mutation).await? else {
        return Ok(true);
    };
    release_branch_lock(connection, key).await?;
    match decline {
        BranchDecline::SnapshotReaped | BranchDecline::SnapshotIneligible => {
            info!(tenant = tenant.log_name(), database = %tenant.database, snapshot_key = key.as_i64(), decision = "deferred", reason = %decline, "PostgreSQL maintenance skipped a Snapshot branch");
        }
        BranchDecline::MissingMaintain => {
            warn!(tenant = tenant.log_name(), database = %tenant.database, snapshot_key = key.as_i64(), decision = "capability-unavailable", reason = %decline, "PostgreSQL maintenance skipped a Snapshot branch");
        }
    }
    Ok(false)
}

/// Why a visit holding `key`'s branch lock declines the branch, `None` when
/// it works in it.
async fn decline(connection: &mut PgConnection, key: SnapshotKey, mutation: MutationPrivilege) -> Result<Option<BranchDecline>, MaintenanceError> {
    let record = snapshot_records(connection, Some(key)).await?.into_iter().next();
    Ok(match record.map(|record| record.eligibility(Utc::now())) {
        None => Some(BranchDecline::SnapshotReaped),
        Some(BranchEligibility::Preparing | BranchEligibility::Expiring) => Some(BranchDecline::SnapshotIneligible),
        Some(BranchEligibility::Eligible) => match mutation {
            MutationPrivilege::NotRequired => None,
            MutationPrivilege::Required => {
                (!capability::branch_maintainable(connection, Branch::Snapshot(key)).await?).then_some(BranchDecline::MissingMaintain)
            }
        },
    })
}

/// Release one Snapshot's branch lock.
async fn release_branch_lock(connection: &mut PgConnection, key: SnapshotKey) -> Result<(), MaintenanceError> {
    if select_bool(connection, Query::select().expr(advisory_unlock(branch_lock_key(key))).to_owned()).await? {
        Ok(())
    } else {
        Err(MaintenanceError::BranchLockNotHeld { snapshot: key })
    }
}

/// Read the control records of every Snapshot, or of `only` that one.
async fn snapshot_records(connection: &mut PgConnection, only: Option<SnapshotKey>) -> Result<Vec<SnapshotRecord>, MaintenanceError> {
    let (sql, values) = snapshot_records_statement(only).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let status = row.try_get::<i16, _>(SnapshotPreparation::Status.to_string().as_str())?;
            Ok(SnapshotRecord {
                key: SnapshotKey::try_from(row.try_get::<i64, _>(Snapshots::SnapshotKey.to_string().as_str())?)?,
                status: SnapshotStatusCode::try_from(status).map_err(MaintenanceError::UnreadableSnapshotStatus)?,
                expires_at: row.try_get(Snapshots::ExpiresAt.to_string().as_str())?,
            })
        })
        .collect()
}

/// Each Snapshot's key, expiry and preparation status, or `only` that one's.
fn snapshot_records_statement(only: Option<SnapshotKey>) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .column((Snapshots::Table, Snapshots::ExpiresAt))
        .column((SnapshotPreparation::Table, SnapshotPreparation::Status))
        .from(Snapshots::Table)
        .join(
            JoinType::InnerJoin,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        );
    if let Some(key) = only {
        statement.and_where(Expr::col((Snapshots::Table, Snapshots::SnapshotKey)).eq(key.as_i64()));
    }
    statement
}

/// Read the churn of every published Snapshot branch.
async fn branch_churn(connection: &mut PgConnection) -> Result<Vec<BranchChurn>, MaintenanceError> {
    let (sql, values) = branch_churn_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let schema = row.try_get::<String, _>(ChurnColumn::Schemaname.to_string().as_str())?;
            Ok(BranchChurn {
                key: published_snapshot_key(&schema).ok_or(MaintenanceError::UnknownBranchSchema)?,
                tuple_changes: non_negative(row.try_get(ChurnColumn::TupleChanges.to_string().as_str())?, CatalogCounter::TupleChanges)?,
            })
        })
        .collect()
}

/// The columns the churn statement projects.
#[derive(Clone, Copy, Iden)]
enum ChurnColumn {
    /// `pg_stat_user_tables.schemaname`.
    Schemaname,
    /// `n_tup_upd + n_tup_del` summed over the schema's tables.
    TupleChanges,
}

/// The rows each published Snapshot branch's tables updated or deleted,
/// grouped by schema. A staging schema never matches the branch pattern.
fn branch_churn_statement() -> SelectStatement {
    let changes = Expr::col(Alias::new("n_tup_upd")).add(Expr::col(Alias::new("n_tup_del")));
    Query::select()
        .column(ChurnColumn::Schemaname)
        .expr_as(cast(Func::sum(changes).into(), PostgresType::Bigint), ChurnColumn::TupleChanges)
        .from(Alias::new("pg_stat_user_tables"))
        .and_where(Expr::col(ChurnColumn::Schemaname).binary(PgBinOper::Regex, Expr::val(PUBLISHED_SNAPSHOT_SCHEMA_PATTERN)))
        .group_by_col(ChurnColumn::Schemaname)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::branch_selection::{BranchChurn, BranchEligibility, SnapshotRecord, branch_churn_statement, churn_order, snapshot_records_statement};
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
    use chrono::{Duration, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn key(raw: i64) -> SnapshotKey {
        SnapshotKey::try_from(raw).expect("positive snapshot key")
    }

    #[test]
    fn snapshot_expiring_within_thirty_minutes_is_not_eligible() {
        let now = Utc::now();
        let record = |status, minutes| SnapshotRecord {
            key: key(7),
            status,
            expires_at: now + Duration::minutes(minutes),
        };
        assert_eq!(record(SnapshotStatusCode::Success, 29).eligibility(now), BranchEligibility::Expiring);
        assert_eq!(record(SnapshotStatusCode::Success, 31).eligibility(now), BranchEligibility::Eligible);
        assert_eq!(record(SnapshotStatusCode::Preparing, 31).eligibility(now), BranchEligibility::Preparing);
        for status in [SnapshotStatusCode::Partial, SnapshotStatusCode::Empty, SnapshotStatusCode::Failure] {
            assert_eq!(record(status, 31).eligibility(now), BranchEligibility::Eligible, "{status:?}");
        }
    }

    #[test]
    fn branch_selection_orders_by_schema_churn() {
        let churn = [
            BranchChurn { key: key(1), tuple_changes: 0 },
            BranchChurn {
                key: key(2),
                tuple_changes: 900,
            },
            BranchChurn {
                key: key(3),
                tuple_changes: 300,
            },
        ];
        assert_eq!(churn_order(&[key(1), key(2), key(3)], &churn), [key(2), key(3)]);
        assert_eq!(churn_order(&[key(1), key(3)], &churn), [key(3)], "an ineligible branch is never selected");
    }

    #[test]
    fn branch_churn_statement_groups_by_schema() {
        let sql = branch_churn_statement().to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "schemaname", SUM("n_tup_upd" + "n_tup_del")::bigint AS "tuple_changes" FROM "pg_stat_user_tables" WHERE "schemaname" ~ '^snap_[1-9][0-9]*$' GROUP BY "schemaname""#
        );
        assert!(!sql.contains("_stg"), "{sql}");
    }

    #[test]
    fn the_control_record_is_read_again_for_one_snapshot() {
        let every = snapshot_records_statement(None).to_string(PostgresQueryBuilder);
        assert_eq!(
            every,
            r#"SELECT "snapshots"."snapshot_key", "snapshots"."expires_at", "snapshot_preparation"."status" FROM "snapshots" INNER JOIN "snapshot_preparation" ON "snapshot_preparation"."snapshot_key" = "snapshots"."snapshot_key""#
        );
        let one = snapshot_records_statement(Some(key(42))).to_string(PostgresQueryBuilder);
        assert_eq!(one, format!(r#"{every} WHERE "snapshots"."snapshot_key" = 42"#));
    }
}
