//! Each density-measured GIN's size as a catalog sample against the size its
//! covered rows take at its keys per row and its bytes per key, over the rows
//! its table's latest analyze counted.

use crate::{
    analyzed_rows::AnalyzedRows,
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    controller::catalog_sampling::{CatalogSample, SampleEvidence},
    error::MaintenanceError,
    gin_density::{self, GinCoverage, GinCoverageReading},
    gin_keys::{self, KeysPerRow},
    inspection::IndexSize,
    policy::PolicyProfile,
    state::{RegisteredTenant, analyzed_rows, gin_key_layout},
};
use sqlx::PgConnection;
use tracing::info;

/// Sample every density-measured GIN of `branch` the catalogs describe, at
/// the bytes per key last measured on it, else its shape's default. A GIN of a
/// shape with no default and nothing measured records the bytes per key it
/// has now and is sampled at them, so its first sample is never degraded.
pub(crate) async fn density_samples(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    sizes: &[IndexSize],
    coverage: &[GinCoverage],
    profile: PolicyProfile,
) -> Result<Vec<CatalogSample>, MaintenanceError> {
    let layouts = gin_key_layout::stored_key_layouts(platform, tenant, branch).await?;
    let mut samples = Vec::new();
    for reading in gin_density::density_readings(sizes, coverage) {
        let known = layouts.key_bytes(reading.column).or(gin_keys::default_key_bytes(reading.column));
        let key_bytes = if let Some(key_bytes) = known {
            key_bytes
        } else {
            let Some(measured) = reading.key_bytes() else {
                continue;
            };
            gin_key_layout::record_key_bytes(platform, tenant, branch, reading.column, Some(measured)).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = reading.column.gin().relation_name(), key_millibytes = measured.thousandths(), keys_per_row_thousandths = reading.keys_per_row.thousandths(), covered_rows = reading.covered_rows, "PostgreSQL maintenance gin bytes per key measured on first observation");
            measured
        };
        samples.extend(reading.sample(key_bytes, profile).map(|degradation| CatalogSample {
            target: MaintenanceTarget::Index(reading.column.gin()),
            evidence: SampleEvidence::Degradation(degradation),
        }));
    }
    Ok(samples)
}

/// The covered rows of every density-measured GIN of `branch` the catalogs
/// describe, as its table's latest analyze counted them, with the keys it
/// holds per row; each reading taken after a newer analyze replaces the stored
/// rows, and the keys are sampled once for the rows of each analyze. A GIN read
/// before its table's first analyze, or whose keys no sample has read, has
/// none.
pub(crate) async fn settled_coverage(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
) -> Result<Vec<GinCoverage>, MaintenanceError> {
    let stored = analyzed_rows::stored_analyzed_rows(platform, tenant, branch).await?;
    let layouts = gin_key_layout::stored_key_layouts(platform, tenant, branch).await?;
    let mut coverage = Vec::new();
    for reading in gin_density::gin_coverage_readings(connection, branch).await? {
        let row = BranchTarget::new(branch, MaintenanceTarget::Index(reading.column.gin()))?;
        let Some(covered) = analyzed_rows::settled_rows(platform, tenant, row, reading.covered, stored.of(row.target())).await? else {
            continue;
        };
        let stored_keys = layouts.keys_per_row(reading.column);
        let keys = if covered.rows == 0 || layouts.keys_analyzed_at(reading.column) == Some(covered.analyzed_at) {
            stored_keys
        } else {
            sampled_keys(platform, connection, tenant, branch, reading, covered).await?.or(stored_keys)
        };
        coverage.extend(keys.map(|keys_per_row| GinCoverage {
            column: reading.column,
            covered_rows: covered.rows,
            keys_per_row,
        }));
    }
    Ok(coverage)
}

/// Sample the keys per row of the GIN `reading` covers over the `covered`
/// rows an analyze counted, and record the sample against that analyze, so
/// the GIN is sampled once per analyze whatever the sample read. `None` when
/// it read no covered row.
async fn sampled_keys(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    reading: GinCoverageReading,
    covered: AnalyzedRows,
) -> Result<Option<KeysPerRow>, MaintenanceError> {
    let sampled = gin_keys::sample_keys_per_row(connection, branch, reading.column, covered.rows, reading.heap_pages).await?;
    gin_key_layout::record_keys_sample(platform, tenant, branch, reading.column, sampled, covered.analyzed_at).await?;
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = reading.column.gin().relation_name(), keys_per_row_thousandths = ?sampled.map(KeysPerRow::thousandths), covered_rows = covered.rows, analyzed_at = %covered.analyzed_at, "PostgreSQL maintenance gin keys per row sampled");
    Ok(sampled)
}
