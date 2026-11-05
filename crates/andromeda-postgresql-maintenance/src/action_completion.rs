//! What a completed action reports, and the follow-up work its kind needs.
//!
//! A completed action logs the readings taken around it. A rebuilt GIN whose
//! density is measured has its bytes per key measured again, a vacuum that
//! could not remove what it was queued for names the holders of the vacuum
//! horizon, and a rebuilt rowstore chunk is recorded so it is never rebuilt
//! again.

use crate::{
    action::target_action,
    action_measurement::{ActionBaseline, ActionComparison},
    action_policy::ActionVerdict,
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, index_target::DensityColumn},
    controller::density_sampling,
    error::MaintenanceError,
    gin_density::{self, DensityReading},
    gin_keys::KeyBytes,
    inspection,
    rowstore_rebuild,
    state::{RegisteredTenant, gin_key_layout},
    vacuum_horizon::{self, HorizonHolder, HorizonScope},
};
use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use std::time::Duration;
use tracing::{info, warn};

/// One completed action, as the log reports it.
pub(crate) struct Completion<'r> {
    /// The target, in its branch.
    pub(crate) row: BranchTarget,
    /// The relation the statement worked on.
    pub(crate) relation: &'r str,
    /// The reading taken before the action.
    pub(crate) baseline: ActionBaseline,
    /// The readings before and after the action.
    pub(crate) comparison: ActionComparison,
    /// Whether the action achieved its purpose.
    pub(crate) verdict: ActionVerdict,
    /// The cooldown its ending set.
    pub(crate) cooldown_until: DateTime<Utc>,
    /// How long the statement ran.
    pub(crate) duration: Duration,
}

/// Log a completed action with the readings around it, and do the follow-up
/// work its kind needs: a rebuilt GIN's bytes per key measured, the holders
/// of the horizon an ineffective vacuum ran into reported, a rebuilt chunk
/// recorded.
pub(crate) async fn report_completion(
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    completion: Completion<'_>,
) -> Result<(), MaintenanceError> {
    let Completion {
        row,
        baseline,
        comparison,
        verdict,
        cooldown_until,
        duration,
        ..
    } = completion;
    let (branch, target) = (row.branch(), row.target());
    let action = target_action(target);
    match comparison {
        ActionComparison::Index { before_bytes, after_bytes } => {
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
            if let MaintenanceTarget::Index(index) = target
                && let Some(column) = index.density_column()
            {
                measure_rebuilt_key_bytes(platform, tenant_connection, tenant, branch, column).await?;
            }
        }
        ActionComparison::Heap { before, after } => {
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), dead_tuples_before = before.dead, dead_tuples_after = after.dead, removed_dead_tuples = before.dead.saturating_sub(after.dead), live_tuples_after = after.live, dead_percent_after = ?comparison.post_vacuum_dead_percent(), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
            if verdict == ActionVerdict::Ineffective {
                report_horizon_holders(tenant_connection, tenant, target).await?;
            }
        }
        ActionComparison::Statistics {
            before_modified,
            after_modified,
        } => {
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, modified_since_analyze_before = before_modified, modified_since_analyze_after = after_modified, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
        }
        ActionComparison::Columnstore { after } => {
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, chunk = completion.relation, partiality_after = ?after, verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
        }
        ActionComparison::Repack { before_bytes, after_bytes } => {
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
        }
        ActionComparison::Rowstore { before_bytes, after_bytes } => {
            if let ActionBaseline::Rowstore { table, range_start, .. } = baseline {
                rowstore_rebuild::record_rebuilt(tenant_connection, table, range_start).await?;
            }
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, chunk = completion.relation, before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
        }
    }
    Ok(())
}

/// Record the bytes per key a rebuilt density-measured GIN takes, read the way
/// a catalog sample reads its size, over its analyzed covered rows and its
/// sampled keys per row. A GIN whose covered rows or keys the platform state
/// does not hold has its bytes per key cleared, so it is judged at its shape's
/// default, or measured on its next observation when its shape has none.
async fn measure_rebuilt_key_bytes(
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    column: DensityColumn,
) -> Result<(), MaintenanceError> {
    let sizes = inspection::index_sizes(tenant_connection, branch).await?;
    let coverage = density_sampling::settled_coverage(platform, tenant_connection, tenant, branch).await?;
    let key_bytes = gin_density::density_readings(&sizes, &coverage)
        .find(|reading| reading.column == column)
        .and_then(DensityReading::key_bytes);
    gin_key_layout::record_key_bytes(platform, tenant, branch, column, key_bytes).await?;
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = column.gin().relation_name(), key_millibytes = ?key_bytes.map(KeyBytes::thousandths), "PostgreSQL maintenance gin bytes per key measured after rebuild");
    Ok(())
}

/// Log every holder of the vacuum horizon after a vacuum that could not
/// remove what it was queued for, so an operator can release the one that
/// held it.
async fn report_horizon_holders(connection: &mut PgConnection, tenant: &RegisteredTenant, target: MaintenanceTarget) -> Result<(), MaintenanceError> {
    let holders = vacuum_horizon::horizon_holders(connection, HorizonScope::TenantDatabase).await?;
    if holders.is_empty() {
        warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), decision = "ineffective", "PostgreSQL vacuum left dead tuples while no horizon holder remains");
    }
    for holder in holders {
        let kind: &'static str = holder.kind().into();
        match holder {
            HorizonHolder::ClientBackend { xmin_age, transaction_seconds } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age, transaction_seconds = ?transaction_seconds, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
            HorizonHolder::PreparedTransaction { xid_age, prepared_seconds } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age = xid_age, prepared_seconds, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
            HorizonHolder::ReplicationSlot {
                xmin_age,
                catalog_xmin_age,
                activity,
            } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age = ?xmin_age, catalog_xmin_age = ?catalog_xmin_age, activity = ?activity, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
            HorizonHolder::StandbyFeedback { xmin_age } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
        }
    }
    Ok(())
}
