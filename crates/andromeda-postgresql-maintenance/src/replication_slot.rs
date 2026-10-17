//! Replication slots keeping WAL on disk.
//!
//! A slot keeps every WAL segment from its `restart_lsn` on. An inactive slot,
//! left by a decommissioned standby, a change-data consumer that went away or a
//! tool, keeps them until the volume fills and the server stops; PostgreSQL
//! bounds this only when `max_slot_wal_keep_size` is set, which it is not by
//! default. `wal_status` says how far a slot has gone: `extended` keeps WAL
//! past `max_wal_size`, `unreserved` is about to lose some at the next
//! checkpoint, `lost` has lost what its consumer still needs. The controller
//! never drops a slot: whether a consumer is gone for good is the operator's
//! decision.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    observation::{Finding, ObservationDecision, ObservationLevel},
};
use andromeda_postgresql_seaquery::maintenance::wal_written_since;
use andromeda_telemetry::maintenance::record_replication_slot_wal;
use sea_query::{Alias, Expr, Iden, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;
use strum::EnumString;

/// How much WAL a slot keeps relative to what the server reserves.
#[derive(Clone, Copy, Debug, EnumString, Eq, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum WalStatus {
    /// Within `max_wal_size`.
    Reserved,
    /// Past `max_wal_size`, kept for the slot.
    Extended,
    /// About to be removed at the next checkpoint.
    Unreserved,
    /// Removed although the slot's consumer still needs it.
    Lost,
}

/// One replication slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReplicationSlot {
    /// Its name, for the log.
    pub(crate) name: String,
    /// `physical` or `logical`, for the log.
    pub(crate) kind: String,
    /// The database a logical slot decodes, `None` for a physical slot.
    pub(crate) database: Option<String>,
    /// Whether a consumer is connected to it.
    pub(crate) active: bool,
    /// How much WAL it keeps, `None` for a slot that never reserved any.
    pub(crate) wal_status: Option<WalStatus>,
    /// The WAL bytes it keeps on disk, `None` when it keeps none.
    pub(crate) retained_bytes: Option<u64>,
    /// The WAL bytes that may still be written before it is lost, `None`
    /// when `max_slot_wal_keep_size` sets no bound or it is lost already.
    pub(crate) safe_wal_size: Option<i64>,
}

impl ReplicationSlot {
    /// The finding the slot reports: a lost slot, or one keeping WAL past
    /// `max_wal_size`, an error once that WAL is about to go.
    pub(crate) const fn finding(&self) -> Option<Finding> {
        match self.wal_status {
            Some(WalStatus::Lost) => Some(Finding::error(ObservationDecision::ReplicationSlotLost)),
            Some(WalStatus::Unreserved) => Some(Finding::error(ObservationDecision::ReplicationSlotRetainingWal)),
            Some(WalStatus::Extended) => Some(Finding::warning(ObservationDecision::ReplicationSlotRetainingWal)),
            Some(WalStatus::Reserved) | None => None,
        }
    }
}

/// Read every replication slot, report the most WAL one keeps, and log each
/// slot that warrants it.
pub(crate) async fn observe_replication_slots(platform: &mut PgConnection) -> Result<(), MaintenanceError> {
    let (sql, values) = replication_slots_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(platform).await?;
    let slots = rows.iter().map(replication_slot_of).collect::<Result<Vec<_>, _>>()?;
    record_replication_slot_wal(slots.iter().filter_map(|slot| slot.retained_bytes).max().unwrap_or_default());
    for slot in &slots {
        if let Some(finding) = slot.finding() {
            observation_event!(finding, slot = slot.name.as_str(), slot_type = slot.kind.as_str(), slot_database = ?slot.database, active = slot.active, wal_status = ?slot.wal_status, retained_bytes = ?slot.retained_bytes, safe_wal_size = ?slot.safe_wal_size, "PostgreSQL replication slot keeps WAL past max_wal_size or has lost WAL its consumer needs");
        }
    }
    Ok(())
}

/// The columns the slot statement projects.
#[derive(Clone, Copy, Iden)]
enum SlotColumn {
    /// `slot_name`.
    SlotName,
    /// `slot_type`.
    SlotType,
    /// `database`.
    Database,
    /// `active`.
    Active,
    /// `wal_status`.
    WalStatus,
    /// `safe_wal_size`.
    SafeWalSize,
    /// The WAL written since `restart_lsn`.
    RetainedBytes,
}

/// Every replication slot of the cluster with the WAL it keeps.
fn replication_slots_statement() -> SelectStatement {
    Query::select()
        .columns([
            SlotColumn::SlotName,
            SlotColumn::SlotType,
            SlotColumn::Database,
            SlotColumn::Active,
            SlotColumn::WalStatus,
            SlotColumn::SafeWalSize,
        ])
        .expr_as(wal_written_since(Expr::col(Alias::new("restart_lsn"))), SlotColumn::RetainedBytes)
        .from(Alias::new("pg_replication_slots"))
        .to_owned()
}

/// Decode one slot row.
fn replication_slot_of(row: &PgRow) -> Result<ReplicationSlot, MaintenanceError> {
    let column = |column: SlotColumn| column.to_string();
    Ok(ReplicationSlot {
        name: row.try_get(column(SlotColumn::SlotName).as_str())?,
        kind: row.try_get(column(SlotColumn::SlotType).as_str())?,
        database: row.try_get(column(SlotColumn::Database).as_str())?,
        active: row.try_get(column(SlotColumn::Active).as_str())?,
        wal_status: row
            .try_get::<Option<String>, _>(column(SlotColumn::WalStatus).as_str())?
            .map(|status| WalStatus::from_str(&status).map_err(MaintenanceError::UnknownWalStatus))
            .transpose()?,
        retained_bytes: row
            .try_get::<Option<i64>, _>(column(SlotColumn::RetainedBytes).as_str())?
            .map(|bytes| non_negative(bytes, CatalogCounter::SlotWalBytes))
            .transpose()?,
        safe_wal_size: row.try_get(column(SlotColumn::SafeWalSize).as_str())?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        observation::{Finding, ObservationDecision},
        replication_slot::{ReplicationSlot, WalStatus, replication_slots_statement},
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::str::FromStr;

    fn slot(wal_status: Option<WalStatus>, active: bool) -> ReplicationSlot {
        ReplicationSlot {
            name: "standby_1".to_owned(),
            kind: "physical".to_owned(),
            database: None,
            active,
            wal_status,
            retained_bytes: wal_status.map(|_| 2_147_483_648),
            safe_wal_size: None,
        }
    }

    #[test]
    fn a_slot_within_max_wal_size_reports_nothing_whether_or_not_it_is_active() {
        assert_eq!(slot(Some(WalStatus::Reserved), false).finding(), None);
        assert_eq!(slot(Some(WalStatus::Reserved), true).finding(), None);
        assert_eq!(slot(None, false).finding(), None);
    }

    #[test]
    fn a_slot_past_max_wal_size_retains_wal_as_a_warning_until_its_wal_is_about_to_go() {
        assert_eq!(
            slot(Some(WalStatus::Extended), false).finding(),
            Some(Finding::warning(ObservationDecision::ReplicationSlotRetainingWal))
        );
        assert_eq!(
            slot(Some(WalStatus::Unreserved), true).finding(),
            Some(Finding::error(ObservationDecision::ReplicationSlotRetainingWal))
        );
    }

    #[test]
    fn a_lost_slot_reports_replication_slot_lost_as_an_error() {
        assert_eq!(
            slot(Some(WalStatus::Lost), false).finding(),
            Some(Finding::error(ObservationDecision::ReplicationSlotLost))
        );
    }

    #[test]
    fn every_wal_status_postgresql_reports_is_read() {
        for (status, read) in [
            ("reserved", WalStatus::Reserved),
            ("extended", WalStatus::Extended),
            ("unreserved", WalStatus::Unreserved),
            ("lost", WalStatus::Lost),
        ] {
            assert_eq!(WalStatus::from_str(status), Ok(read));
        }
        assert!(WalStatus::from_str("unknown").is_err());
    }

    #[test]
    fn the_slot_statement_measures_the_wal_kept_since_each_restart_position() {
        let sql = replication_slots_statement().to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "slot_name", "slot_type", "database", "active", "wal_status", "safe_wal_size", "#,
                r#"pg_wal_lsn_diff(pg_current_wal_lsn(), "restart_lsn")::bigint AS "retained_bytes" FROM "pg_replication_slots""#
            )
        );
    }
}
