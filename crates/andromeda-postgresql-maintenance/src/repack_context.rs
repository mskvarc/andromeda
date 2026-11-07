//! What decides a repack beyond its sample, read where a pass samples it and
//! again where the repack task is about to run it.

use crate::{
    branch::{Branch, BranchTarget},
    branch_selection,
    capability::{self, MaintainPrivilege, MaintainRight, RepackCapability},
    error::MaintenanceError,
    freeze_policy::FreezePressure,
    repack::table_lock::{RepackTableLock, TableLockTaken},
    repack_policy::{RepackContext, RepackRunState, repack_mode},
};
use andromeda_configuration::{
    human_byte_size::HumanByteSize,
    schema::{
        ceiling::Ceiling,
        database::maintenance::{MaintenanceMode, RepackPolicy},
    },
};
use andromeda_postgresql_schema::branch_schema_version::BranchSchemaVersion;
use andromeda_postgresql_seaquery::{repack_catalog::triggered_tables, statement::Statement};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::PgConnection;

/// What one tenant visit, or one run's preparation, knows of every repack it
/// decides: the configuration, the extension, and the cluster's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RepackInputs {
    /// Whether automatic mode runs repacks or only reports them.
    pub(crate) policy: RepackPolicy,
    /// The largest table a repack copies.
    pub(crate) max_size: Ceiling<HumanByteSize>,
    /// Whether a repack may run in the tenant database.
    pub(crate) capability: RepackCapability,
    /// The most WAL any replication slot keeps on disk.
    pub(crate) retained_wal_bytes: u64,
    /// The tenant database's freeze pressure.
    pub(crate) freeze: FreezePressure,
    /// The highest branch migration the binary embeds.
    pub(crate) branch_version: BranchSchemaVersion,
}

impl RepackInputs {
    /// The configured largest relation a repack copies, `None` when
    /// unlimited.
    pub(crate) fn max_relation_bytes(self) -> Option<u64> {
        self.max_size.limit().map(HumanByteSize::as_u64)
    }
}

/// The mode a repack of `row` is decided under in a branch decided under
/// `branch_mode`: observe unless the branch is automatic and repacks are
/// performed, and observe when the extension cannot run a repack or the role
/// does not own the table.
pub(crate) async fn repack_decision_mode(
    connection: &mut PgConnection,
    row: BranchTarget,
    branch_mode: MaintenanceMode,
    inputs: RepackInputs,
) -> Result<MaintenanceMode, MaintenanceError> {
    let mode = repack_mode(branch_mode, inputs.policy);
    if mode != MaintenanceMode::Automatic {
        return Ok(mode);
    }
    match inputs.capability {
        RepackCapability::Ready(_) => {}
        RepackCapability::MissingExtension | RepackCapability::UnsupportedVersion(_) | RepackCapability::MissingRight => return Ok(MaintenanceMode::Observe),
    }
    let relation = row.branch().qualified(row.target().relation_name());
    Ok(match capability::ownership(connection, &relation).await? {
        MaintainPrivilege::Held => MaintenanceMode::Automatic,
        MaintainPrivilege::NotGranted => MaintenanceMode::Observe,
    })
}

/// Why automatic mode only observes a repack whose decision mode
/// [`repack_decision_mode`] lowered: the capability the extension or the role
/// lacks, or else the table's ownership.
pub(crate) fn withheld_reason(capability: RepackCapability) -> &'static str {
    capability.missing().unwrap_or_else(|| MaintainRight::Ownership.into())
}

/// The context a repack of `row`, the table `relid`, is decided in at `now`.
pub(crate) async fn repack_context(
    connection: &mut PgConnection,
    row: BranchTarget,
    relid: u32,
    inputs: RepackInputs,
    now: DateTime<Utc>,
) -> Result<RepackContext, MaintenanceError> {
    let snapshot_remaining = match row.branch() {
        Branch::Live => None,
        Branch::Snapshot(key) => Some(
            branch_selection::snapshot_expiry(connection, key, inputs.branch_version)
                .await?
                .map_or(ChronoDuration::zero(), |expires_at| expires_at.signed_duration_since(now)),
        ),
    };
    Ok(RepackContext {
        max_relation_bytes: inputs.max_relation_bytes(),
        retained_wal_bytes: inputs.retained_wal_bytes,
        snapshot_remaining,
        freeze: inputs.freeze,
        run: run_state(connection, relid).await?,
    })
}

/// Whether a run of the table `relid` is present: its trigger installed, and
/// its table's lock held by a running run or by nobody.
pub(crate) async fn run_state(connection: &mut PgConnection, relid: u32) -> Result<RepackRunState, MaintenanceError> {
    let triggered = triggered_tables().to_query_scalar::<i64>().fetch_all(&mut *connection).await?;
    if !triggered.contains(&i64::from(relid)) {
        return Ok(RepackRunState::Idle);
    }
    Ok(match RepackTableLock::of(relid).probe(connection).await? {
        TableLockTaken::Taken => RepackRunState::Leftover,
        TableLockTaken::HeldElsewhere => RepackRunState::Running,
    })
}
