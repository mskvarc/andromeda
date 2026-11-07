//! A visit's free-space probe of one heap, and the repack decision it feeds.
//!
//! The probe estimates the heap's and its TOAST table's free space and
//! decides the heap's repack target on it. A visit only ever queues a
//! repack: the repack task runs it, after judging it again on a fresh
//! sample. A repack whose right the role lacks, because the extension cannot
//! run one or the role does not own the table, is decided under observe mode,
//! and a repack queued before is withdrawn.

use crate::{
    action,
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, heap_target::HeapTarget},
    controller::{idle_gates, probe_selection::HeapSize},
    error::MaintenanceError,
    heap_free_space,
    policy::{PolicyDecision, PolicyProfile, lapse},
    repack_context::{self, RepackInputs},
    repack_policy::{RepackSample, decide_repack, repack_mode},
    state::{RegisteredTenant, pending_action, target_record},
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use chrono::Utc;
use sqlx::PgConnection;
use tracing::info;

/// The heaps of `branch` past the repack size floor, with their bytes.
pub(crate) async fn heap_sizes(connection: &mut PgConnection, branch: Branch) -> Result<Vec<HeapSize>, MaintenanceError> {
    let mut sizes = Vec::new();
    for heap in branch.heaps() {
        if let Some(bytes) = heap_free_space::heap_table_bytes(connection, branch, heap).await? {
            sizes.push(HeapSize { branch, heap, bytes });
        }
    }
    Ok(sizes)
}

/// Probe one heap and decide its repack under the mode of its branch,
/// `branch_mode`, returning the decision, `None` when the heap no longer
/// resolves.
pub(crate) async fn probe_heap(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    size: HeapSize,
    branch_mode: MaintenanceMode,
    inputs: RepackInputs,
    profile: PolicyProfile,
) -> Result<Option<PolicyDecision>, MaintenanceError> {
    let HeapSize { branch, heap, .. } = size;
    let Some(reading) = heap_free_space::heap_free_space(connection, branch, heap).await? else {
        return Ok(None);
    };
    let row = BranchTarget::new(branch, MaintenanceTarget::Repack(heap))?;
    let mode = repack_context::repack_decision_mode(connection, row, branch_mode, inputs).await?;
    if mode != repack_mode(branch_mode, inputs.policy) {
        report_withheld(platform, tenant, row, heap, inputs).await?;
    }
    let now = Utc::now();
    let context = repack_context::repack_context(connection, row, reading.relid, inputs, now).await?;
    let sample = RepackSample::of_reading(reading, profile);
    let state = target_record::load_target_state(platform, tenant, row).await?;
    let decision = decide_repack(mode, profile, state, sample, context, idle_gates(), now);
    match lapse(state, decision) {
        Some(reason) => action::withdraw_lapsed(platform, tenant, row, reason).await?,
        None => target_record::record_decision(platform, tenant, row, Some(sample.degradation), decision, state.breach, now).await?,
    }
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = heap.relation_name(), relation_bytes = reading.relation_bytes, free_bytes = reading.free_bytes, free_percent = sample.degradation.degraded_percent, dead_percent = reading.dead_percent, run = ?context.run, decision = ?decision, "PostgreSQL maintenance heap free-space probe completed");
    Ok(Some(decision))
}

/// Log why automatic mode only observes a repack, and withdraw one queued
/// before.
async fn report_withheld(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    heap: HeapTarget,
    inputs: RepackInputs,
) -> Result<(), MaintenanceError> {
    let reason = repack_context::withheld_reason(inputs.capability);
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = heap.relation_name(), decision = "capability-unavailable", reason, "PostgreSQL maintenance only observes a repack the extension or the role cannot run");
    for withdrawn in pending_action::withdraw_action(platform, tenant, row).await? {
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = withdrawn.relation_name(), decision = "would-act", "PostgreSQL maintenance withdrew a queued repack whose right the role lacks");
    }
    Ok(())
}
