//! Each density-measured GIN's density as a catalog sample against its
//! recorded baseline.

use crate::{
    branch::Branch,
    candidate::MaintenanceTarget,
    controller::catalog_sampling::CatalogSample,
    error::MaintenanceError,
    gin_density::{self, GinCoverage},
    inspection::IndexSize,
    policy::PolicyProfile,
    state::{RegisteredTenant, density_baseline},
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
            degradation,
        }));
    }
    Ok(samples)
}
