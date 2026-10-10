//! Who holds back the tenant database's vacuum horizon, and whether any of
//! them will not let go on its own.
//!
//! A vacuum removes only tuples that died before the oldest transaction ID
//! any holder still needs. Client transactions are one kind of holder; a
//! prepared transaction, a replication slot carrying an `xmin` and a
//! standby's hot-standby feedback hold the horizon just as firmly without
//! appearing as an open client transaction.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    introspection::current_database,
    maintenance::{backend_pid, transaction_age},
    pg_type::PostgresType,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Iden, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr, UnionType};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;
use strum::{EnumString, IntoStaticStr};

/// A prepared transaction younger than this is presumed to be inside an
/// ordinary two-phase commit that its coordinator is about to finish.
const PREPARED_TRANSACTION_GRACE_SECONDS: u64 = 30;

/// The kind of one vacuum horizon holder.
#[derive(Clone, Copy, Debug, EnumString, IntoStaticStr, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum HorizonHolderKind {
    /// A client backend of the tenant database holding a snapshot.
    ClientBackend,
    /// A prepared transaction of the tenant database.
    PreparedTransaction,
    /// A replication slot that is physical or belongs to the tenant database.
    ReplicationSlot,
    /// A standby's hot-standby feedback, reported through its WAL sender.
    StandbyFeedback,
}

/// Whether a replication slot has a connected consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotActivity {
    /// A consumer is connected and normally advances the slot.
    Active,
    /// No consumer is connected; nothing advances the slot.
    Inactive,
}

/// One holder of the tenant database's vacuum horizon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HorizonHolder {
    /// A client backend whose snapshot holds the horizon.
    ClientBackend {
        /// Age in transactions of the backend's `backend_xmin`.
        xmin_age: u64,
        /// Seconds since its transaction started, `None` when PostgreSQL
        /// reports no transaction start.
        transaction_seconds: Option<u64>,
    },
    /// A prepared transaction, which holds the horizon until it is committed
    /// or rolled back by name.
    PreparedTransaction {
        /// Age in transactions of its transaction ID.
        xid_age: u64,
        /// Seconds since it was prepared.
        prepared_seconds: u64,
    },
    /// A replication slot.
    ReplicationSlot {
        /// Age in transactions of the slot's `xmin`, `None` when it holds none.
        xmin_age: Option<u64>,
        /// Age in transactions of the slot's `catalog_xmin`, which holds back
        /// only system catalogs, `None` when it holds none.
        catalog_xmin_age: Option<u64>,
        /// Whether a consumer is connected.
        activity: SlotActivity,
    },
    /// A standby whose feedback holds the horizon.
    StandbyFeedback {
        /// Age in transactions of the WAL sender's `backend_xmin`.
        xmin_age: u64,
    },
}

impl HorizonHolder {
    /// The holder's kind.
    pub(crate) const fn kind(self) -> HorizonHolderKind {
        match self {
            HorizonHolder::ClientBackend { .. } => HorizonHolderKind::ClientBackend,
            HorizonHolder::PreparedTransaction { .. } => HorizonHolderKind::PreparedTransaction,
            HorizonHolder::ReplicationSlot { .. } => HorizonHolderKind::ReplicationSlot,
            HorizonHolder::StandbyFeedback { .. } => HorizonHolderKind::StandbyFeedback,
        }
    }
}

/// The kind of the first holder that will not release the horizon on its own,
/// `None` when every holder normally advances.
///
/// A prepared transaction past its grace period waits for an operator, and an
/// inactive slot with an `xmin` has no consumer to advance it. A client
/// transaction is the long-transaction gate's concern, and an active slot and
/// standby feedback normally advance, so they are reported, never gated on.
pub(crate) fn horizon_held_by(holders: &[HorizonHolder]) -> Option<HorizonHolderKind> {
    holders
        .iter()
        .find(|holder| match holder {
            HorizonHolder::PreparedTransaction { prepared_seconds, .. } => *prepared_seconds > PREPARED_TRANSACTION_GRACE_SECONDS,
            HorizonHolder::ReplicationSlot {
                xmin_age: Some(_),
                activity: SlotActivity::Inactive,
                ..
            } => true,
            HorizonHolder::ClientBackend { .. }
            | HorizonHolder::ReplicationSlot { xmin_age: None, .. }
            | HorizonHolder::ReplicationSlot {
                activity: SlotActivity::Active,
                ..
            }
            | HorizonHolder::StandbyFeedback { .. } => false,
        })
        .map(|holder| holder.kind())
}

/// Read every holder of the tenant database's vacuum horizon.
pub(crate) async fn horizon_holders(connection: &mut PgConnection) -> Result<Vec<HorizonHolder>, MaintenanceError> {
    let (sql, values) = horizon_holders_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    let now = Utc::now();
    rows.iter().map(|row| holder_of(row, now)).collect()
}

/// The output columns every arm of the holder statement projects.
#[derive(Clone, Copy, Iden)]
enum HolderColumn {
    /// [`HorizonHolderKind`] label.
    HolderKind,
    /// Age of the held `xmin` or transaction ID.
    XminAge,
    /// Age of a slot's `catalog_xmin`.
    CatalogXminAge,
    /// Transaction start or prepare time.
    HeldSince,
    /// Whether a slot is active.
    IsActive,
}

/// One statement listing every holder, each arm projecting the same columns.
///
/// Client backends and prepared transactions are pinned to the current
/// database, because only they hold this database's horizon. A physical slot
/// (`database` is null) and standby feedback hold every database's horizon.
/// The maintenance session itself holds a snapshot while it reads and is
/// excluded.
fn horizon_holders_statement() -> SelectStatement {
    let mut statement = holder_arm(
        HorizonHolderKind::ClientBackend,
        transaction_age(Expr::col(Alias::new("backend_xmin"))),
        typed_null(PostgresType::Bigint),
        Expr::col(Alias::new("xact_start")),
        typed_null(PostgresType::Boolean),
    )
    .from(Alias::new("pg_stat_activity"))
    .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
    .and_where(Expr::col(Alias::new("backend_type")).eq("client backend"))
    .and_where(Expr::col(Alias::new("backend_xmin")).is_not_null())
    .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
    .to_owned();
    let prepared = holder_arm(
        HorizonHolderKind::PreparedTransaction,
        transaction_age(Expr::col(Alias::new("transaction"))),
        typed_null(PostgresType::Bigint),
        Expr::col(Alias::new("prepared")),
        typed_null(PostgresType::Boolean),
    )
    .from(Alias::new("pg_prepared_xacts"))
    .and_where(Expr::col(Alias::new("database")).eq(current_database()))
    .to_owned();
    let slots = holder_arm(
        HorizonHolderKind::ReplicationSlot,
        transaction_age(Expr::col(Alias::new("xmin"))),
        transaction_age(Expr::col(Alias::new("catalog_xmin"))),
        typed_null(PostgresType::Timestamptz),
        Expr::col(Alias::new("active")),
    )
    .from(Alias::new("pg_replication_slots"))
    .and_where(
        Expr::col(Alias::new("xmin"))
            .is_not_null()
            .or(Expr::col(Alias::new("catalog_xmin")).is_not_null()),
    )
    .and_where(
        Expr::col(Alias::new("database"))
            .is_null()
            .or(Expr::col(Alias::new("database")).eq(current_database())),
    )
    .to_owned();
    let standbys = holder_arm(
        HorizonHolderKind::StandbyFeedback,
        transaction_age(Expr::col(Alias::new("backend_xmin"))),
        typed_null(PostgresType::Bigint),
        typed_null(PostgresType::Timestamptz),
        typed_null(PostgresType::Boolean),
    )
    .from(Alias::new("pg_stat_replication"))
    .and_where(Expr::col(Alias::new("backend_xmin")).is_not_null())
    .to_owned();
    statement
        .union(UnionType::All, prepared)
        .union(UnionType::All, slots)
        .union(UnionType::All, standbys);
    statement
}

/// One arm's projection, in the column order every arm shares.
fn holder_arm(kind: HorizonHolderKind, xmin_age: SimpleExpr, catalog_xmin_age: SimpleExpr, held_since: SimpleExpr, is_active: SimpleExpr) -> SelectStatement {
    let label: &'static str = kind.into();
    Query::select()
        .expr_as(Expr::val(label), HolderColumn::HolderKind)
        .expr_as(xmin_age, HolderColumn::XminAge)
        .expr_as(catalog_xmin_age, HolderColumn::CatalogXminAge)
        .expr_as(held_since, HolderColumn::HeldSince)
        .expr_as(is_active, HolderColumn::IsActive)
        .to_owned()
}

/// Decode one row. A column its arm always fills is read as non-null, so a
/// missing value fails the read rather than inventing one.
fn holder_of(row: &PgRow, now: DateTime<Utc>) -> Result<HorizonHolder, MaintenanceError> {
    let kind =
        HorizonHolderKind::from_str(row.try_get::<&str, _>(HolderColumn::HolderKind.to_string().as_str())?).map_err(MaintenanceError::UnknownHorizonHolder)?;
    Ok(match kind {
        HorizonHolderKind::ClientBackend => HorizonHolder::ClientBackend {
            xmin_age: required_age(row, HolderColumn::XminAge)?,
            transaction_seconds: row
                .try_get::<Option<DateTime<Utc>>, _>(HolderColumn::HeldSince.to_string().as_str())?
                .map(|started| seconds_since(started, now)),
        },
        HorizonHolderKind::PreparedTransaction => HorizonHolder::PreparedTransaction {
            xid_age: required_age(row, HolderColumn::XminAge)?,
            prepared_seconds: seconds_since(row.try_get(HolderColumn::HeldSince.to_string().as_str())?, now),
        },
        HorizonHolderKind::ReplicationSlot => HorizonHolder::ReplicationSlot {
            xmin_age: optional_age(row, HolderColumn::XminAge)?,
            catalog_xmin_age: optional_age(row, HolderColumn::CatalogXminAge)?,
            activity: if row.try_get::<bool, _>(HolderColumn::IsActive.to_string().as_str())? {
                SlotActivity::Active
            } else {
                SlotActivity::Inactive
            },
        },
        HorizonHolderKind::StandbyFeedback => HorizonHolder::StandbyFeedback {
            xmin_age: required_age(row, HolderColumn::XminAge)?,
        },
    })
}

/// A transaction age the row's arm always fills.
fn required_age(row: &PgRow, column: HolderColumn) -> Result<u64, MaintenanceError> {
    non_negative(row.try_get(column.to_string().as_str())?, CatalogCounter::TransactionAge)
}

/// A transaction age that is null when the holder holds no such ID.
fn optional_age(row: &PgRow, column: HolderColumn) -> Result<Option<u64>, MaintenanceError> {
    row.try_get::<Option<i64>, _>(column.to_string().as_str())?
        .map(|age| non_negative(age, CatalogCounter::TransactionAge))
        .transpose()
}

/// Whole seconds from `since` to `now`, zero when the database clock runs
/// ahead of the broker's.
fn seconds_since(since: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    now.signed_duration_since(since).num_seconds().clamp(0, i64::MAX).unsigned_abs()
}

#[cfg(test)]
mod tests {
    use crate::vacuum_horizon::{HorizonHolder, HorizonHolderKind, SlotActivity, horizon_held_by, horizon_holders_statement};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn prepared_transaction_older_than_thirty_seconds_holds_the_horizon() {
        let prepared = |prepared_seconds| HorizonHolder::PreparedTransaction { xid_age: 5, prepared_seconds };
        assert_eq!(horizon_held_by(&[prepared(31)]), Some(HorizonHolderKind::PreparedTransaction));
        assert_eq!(horizon_held_by(&[prepared(30)]), None);
    }

    #[test]
    fn inactive_slot_with_xmin_holds_the_horizon_and_active_slot_does_not() {
        let slot = |activity| HorizonHolder::ReplicationSlot {
            xmin_age: Some(10),
            catalog_xmin_age: None,
            activity,
        };
        assert_eq!(horizon_held_by(&[slot(SlotActivity::Inactive)]), Some(HorizonHolderKind::ReplicationSlot));
        assert_eq!(horizon_held_by(&[slot(SlotActivity::Active)]), None);
    }

    #[test]
    fn catalog_xmin_alone_never_defers() {
        let slot = HorizonHolder::ReplicationSlot {
            xmin_age: None,
            catalog_xmin_age: Some(1_000_000),
            activity: SlotActivity::Inactive,
        };
        assert_eq!(horizon_held_by(&[slot]), None);
    }

    #[test]
    fn client_transactions_and_standby_feedback_are_reported_but_never_defer() {
        let holders = [
            HorizonHolder::ClientBackend {
                xmin_age: 1_000,
                transaction_seconds: Some(3_600),
            },
            HorizonHolder::StandbyFeedback { xmin_age: 1_000 },
        ];
        assert_eq!(horizon_held_by(&holders), None);
    }

    #[test]
    fn horizon_statement_is_scoped_to_the_current_database() {
        let sql = horizon_holders_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_stat_activity" WHERE "datname" = current_database()"#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_prepared_xacts" WHERE "database" = current_database()"#), "{sql}");
        assert!(sql.contains(r#""database" IS NULL OR "database" = current_database()"#), "{sql}");
        assert!(sql.contains(r#""xmin" IS NOT NULL OR "catalog_xmin" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_stat_replication" WHERE "backend_xmin" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""pid" <> pg_backend_pid()"#), "{sql}");
        assert_eq!(sql.matches("UNION ALL").count(), 3, "{sql}");
    }
}
