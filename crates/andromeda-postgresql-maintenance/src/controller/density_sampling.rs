//! Each density-measured GIN's density as a catalog sample against its
//! recorded baseline, over the rows its table's latest analyze counted.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    controller::catalog_sampling::{CatalogSample, SampleEvidence},
    error::MaintenanceError,
    gin_density::{self, GinCoverage},
    inspection::IndexSize,
    policy::PolicyProfile,
    state::{RegisteredTenant, analyzed_rows, density_baseline},
};
use sqlx::PgConnection;
use tracing::info;

/// Sample every density-measured GIN of `branch` the catalogs describe. A GIN
/// with no recorded baseline records its current density as one and is
/// sampled against it, so its first sample is never degraded.
pub(crate) async fn density_samples(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    sizes: &[IndexSize],
    coverage: &[GinCoverage],
    profile: PolicyProfile,
) -> Result<Vec<CatalogSample>, MaintenanceError> {
    let baselines = density_baseline::density_baselines(platform, tenant, branch).await?;
    let mut samples = Vec::new();
    for reading in gin_density::density_readings(sizes, coverage) {
        let Some(current) = reading.density() else {
            continue;
        };
        let recorded = baselines
            .iter()
            .find(|baseline| baseline.target == reading.target)
            .map(|baseline| baseline.density);
        let baseline = if let Some(baseline) = recorded {
            baseline
        } else {
            density_baseline::record_density_baseline(platform, tenant, branch, reading.target, Some(current)).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = reading.target.relation_name(), baseline_millibytes_per_row = current.millibytes_per_row(), covered_rows = reading.covered_rows, "PostgreSQL maintenance density baseline recorded");
            current
        };
        samples.extend(reading.sample(baseline, profile).map(|degradation| CatalogSample {
            target: MaintenanceTarget::Index(reading.target),
            evidence: SampleEvidence::Degradation(degradation),
        }));
    }
    Ok(samples)
}

/// The covered rows of every density-measured GIN of `branch` the catalogs
/// describe, as its table's latest analyze counted them; each reading taken
/// after a newer analyze replaces the stored rows. A GIN read before its
/// table's first analyze has none.
pub(crate) async fn settled_coverage(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
) -> Result<Vec<GinCoverage>, MaintenanceError> {
    let stored = analyzed_rows::stored_analyzed_rows(platform, tenant, branch).await?;
    let mut coverage = Vec::new();
    for reading in gin_density::gin_coverage_readings(connection, branch).await? {
        let row = BranchTarget::new(branch, MaintenanceTarget::Index(reading.target))?;
        if let Some(covered) = analyzed_rows::settled_rows(platform, tenant, row, reading.covered, stored.of(row.target())).await? {
            coverage.push(GinCoverage {
                target: reading.target,
                covered_rows: covered.rows,
            });
        }
    }
    Ok(coverage)
}
