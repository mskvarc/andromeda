//! The cheap catalog evidence of every branch a visit works in, the heaps
//! autovacuum has missed there, and the index probes it leaves to the probe
//! budget.

use crate::{
    autovacuum_health,
    branch::Branch,
    btree_estimate,
    controller::{
        PassOutcome,
        catalog_sampling::{self, CatalogSample},
        density_sampling,
        probe_selection,
    },
    error::MaintenanceError,
    gin_density,
    inspection::{self, IndexSize},
    policy::PolicyProfile,
    state::{RegisteredTenant, target_record},
};
use andromeda_postgresql_context_store::maintenance_config::MaintenanceMode;
use andromeda_telemetry::maintenance::record_overdue_relations;
use chrono::Utc;
use sqlx::PgConnection;

/// What sampling a visit's branches found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BranchSampling {
    /// [`PassOutcome::Queued`] if any sample queued work, else
    /// [`PassOutcome::WouldAct`] if any would act, else `None`.
    pub(crate) flagged: Option<PassOutcome>,
    /// The indexes to probe: each branch's in its probe order, the branches in
    /// the order they were sampled.
    pub(crate) probe_candidates: Vec<IndexSize>,
}

/// Record the catalog samples of every branch in `branches`, in order, report
/// the heaps autovacuum has missed in each, and collect each branch's probe
/// candidates.
pub(crate) async fn sample_branches(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branches: &[Branch],
    profile: PolicyProfile,
) -> Result<BranchSampling, MaintenanceError> {
    let mut sampling = BranchSampling {
        flagged: None,
        probe_candidates: Vec::new(),
    };
    let mut overdue_relations = 0;
    for &branch in branches {
        overdue_relations += autovacuum_health::report_overdue_heaps(connection, tenant, branch).await?;
        let index_sizes = inspection::index_sizes(connection, branch).await?;
        let estimates = btree_estimate::btree_estimates(connection, branch).await?;
        let heap_samples = inspection::heap_samples(connection, branch, profile).await?;
        let coverage = gin_density::gin_coverage(connection, branch).await?;
        let probe_states = target_record::index_probe_states(platform, tenant, branch).await?;
        let density_samples = density_sampling::density_samples(platform, tenant, branch, &index_sizes, &coverage, profile).await?;
        let samples = heap_samples.iter().map(CatalogSample::of_heap).chain(density_samples).collect::<Vec<_>>();
        let flagged = catalog_sampling::record_catalog_samples(mode, platform, tenant, branch, &samples, profile).await?;
        sampling.flagged = strongest(sampling.flagged, flagged);
        sampling
            .probe_candidates
            .extend(probe_selection::probe_order(&index_sizes, &probe_states, &estimates, profile, Utc::now()));
    }
    record_overdue_relations(overdue_relations);
    Ok(sampling)
}

/// The flag of two branches' samples together: queued work outranks work
/// that would act.
fn strongest(left: Option<PassOutcome>, right: Option<PassOutcome>) -> Option<PassOutcome> {
    if left == Some(PassOutcome::Queued) || right == Some(PassOutcome::Queued) {
        Some(PassOutcome::Queued)
    } else {
        left.or(right)
    }
}

#[cfg(test)]
mod tests {
    use crate::controller::{PassOutcome, branch_sampling::strongest};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_branch_that_queued_work_outranks_one_that_would_act() {
        assert_eq!(strongest(Some(PassOutcome::WouldAct), Some(PassOutcome::Queued)), Some(PassOutcome::Queued));
        assert_eq!(strongest(Some(PassOutcome::Queued), None), Some(PassOutcome::Queued));
        assert_eq!(strongest(None, Some(PassOutcome::WouldAct)), Some(PassOutcome::WouldAct));
        assert_eq!(strongest(None, None), None);
    }
}
