//! Each heap's dead share as a catalog sample, weighed against the live rows
//! its latest analyze counted.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, heap_target::HeapTarget},
    error::MaintenanceError,
    heap_reading::{self, HeapReading, HeapSample},
    policy::PolicyProfile,
    state::{
        RegisteredTenant,
        analyzed_rows::{self, StoredAnalyzedRows},
    },
};
use sqlx::PgConnection;

/// Sample every allowlisted heap `branch` holds. A heap read before its
/// first analyze has no sample.
pub(crate) async fn heap_samples(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    profile: PolicyProfile,
) -> Result<Vec<HeapSample>, MaintenanceError> {
    let stored = analyzed_rows::stored_analyzed_rows(platform, tenant, branch).await?;
    let mut samples = Vec::new();
    for reading in heap_reading::heap_readings(connection, branch).await? {
        samples.extend(settled_sample(platform, tenant, branch, reading, &stored, profile).await?);
    }
    Ok(samples)
}

/// Sample one heap in `branch`, as [`heap_samples`] samples every heap.
pub(crate) async fn heap_sample(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    heap: HeapTarget,
    profile: PolicyProfile,
) -> Result<Option<HeapSample>, MaintenanceError> {
    let stored = analyzed_rows::stored_analyzed_rows(platform, tenant, branch).await?;
    let reading = heap_reading::heap_reading(connection, branch, heap).await?;
    settled_sample(platform, tenant, branch, reading, &stored, profile).await
}

/// `reading` weighed against the heap's analyzed rows, replacing the stored
/// rows when it was taken after a newer analyze.
async fn settled_sample(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    reading: HeapReading,
    stored: &StoredAnalyzedRows,
    profile: PolicyProfile,
) -> Result<Option<HeapSample>, MaintenanceError> {
    let row = BranchTarget::new(branch, MaintenanceTarget::Heap(reading.target))?;
    let live = analyzed_rows::settled_rows(platform, tenant, row, reading.live, stored.of(row.target())).await?;
    Ok(live.map(|live| reading.sample(live, profile)))
}
