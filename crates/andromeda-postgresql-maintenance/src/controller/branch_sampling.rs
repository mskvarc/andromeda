//! The cheap catalog evidence of every branch a visit works in, the heaps
//! autovacuum has missed there, the freshness of their statistics, and the
//! index probes it leaves to the probe budget.

use crate::{
    autovacuum_health,
    branch::Branch,
    btree_estimate,
    columnstore_partiality,
    controller::{
        PassOutcome,
        action_rights::{self, ActionRights},
        catalog_sampling::{self, CatalogSample},
        density_sampling,
        heap_sampling,
        probe_selection,
        visit_branches::VisitBranches,
    },
    error::MaintenanceError,
    inspection::{self, IndexSize},
    observation::ObservationLevel,
    observed_heap::WatchedHeap,
    policy::PolicyProfile,
    rowstore_rebuild,
    state::{RegisteredTenant, target_record},
    statistics_freshness::{self, HeapStatistics},
};
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
    /// The actions of every branch whose right the role lacks.
    pub(crate) rights: ActionRights,
}

/// Record the catalog samples of every branch in `branches`, in order and
/// each under its own mode, every action whose right the role lacks under
/// observe mode, report the heaps autovacuum has missed in each, and collect
/// each branch's probe candidates.
pub(crate) async fn sample_branches(
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branches: &VisitBranches,
    profile: PolicyProfile,
) -> Result<BranchSampling, MaintenanceError> {
    let mut sampling = BranchSampling {
        flagged: None,
        probe_candidates: Vec::new(),
        rights: ActionRights::default(),
    };
    let mut overdue_relations = 0;
    for visit in branches.iter() {
        let branch = visit.branch;
        overdue_relations += autovacuum_health::report_overdue_heaps(connection, tenant, branch).await?;
        let index_sizes = inspection::index_sizes(connection, branch).await?;
        let estimates = btree_estimate::btree_estimates(connection, branch).await?;
        let heap_samples = heap_sampling::heap_samples(platform, connection, tenant, branch, profile).await?;
        let statistics = statistics_freshness::heap_statistics(connection, branch).await?;
        let partiality = columnstore_partiality::partiality_samples(connection, branch).await?;
        let rebuilds = rowstore_rebuild::rebuild_samples(connection, branch, profile).await?;
        let rights = action_rights::history_rights(platform, connection, tenant, visit, &partiality, &rebuilds, profile).await?;
        let coverage = density_sampling::settled_coverage(platform, connection, tenant, branch).await?;
        let probe_states = target_record::index_probe_states(platform, tenant, branch).await?;
        let density_samples = density_sampling::density_samples(platform, tenant, branch, &index_sizes, &coverage, profile).await?;
        report_stale_observed_heaps(tenant, branch, &statistics);
        let samples = heap_samples
            .iter()
            .map(CatalogSample::of_heap)
            .chain(statistics.into_iter().filter_map(|statistics| match statistics.heap {
                WatchedHeap::Target(heap) => Some(CatalogSample::of_statistics(heap, statistics)),
                WatchedHeap::Observed(_) => None,
            }))
            .chain(density_samples)
            .chain(partiality.into_iter().map(CatalogSample::of_partiality))
            .chain(rebuilds.into_iter().map(CatalogSample::of_rebuild))
            .collect::<Vec<_>>();
        let flagged = catalog_sampling::record_catalog_samples(visit, &rights, platform, tenant, &samples, profile).await?;
        sampling.flagged = strongest(sampling.flagged, flagged);
        sampling.rights.extend(rights);
        sampling
            .probe_candidates
            .extend(probe_selection::probe_order(&index_sizes, &probe_states, &estimates, profile, Utc::now()));
    }
    record_overdue_relations(overdue_relations);
    Ok(sampling)
}

/// Log every heap of `branch` the controller only observes whose planner
/// statistics autoanalyze has missed.
fn report_stale_observed_heaps(tenant: &RegisteredTenant, branch: Branch, statistics: &[HeapStatistics]) {
    let now = Utc::now();
    for heap in statistics {
        if let Some(finding) = heap.observation(now) {
            observation_event!(finding, tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, relation = heap.heap.relation_name(), modified_since_analyze = heap.modified_since_analyze, analyze_trigger = heap.analyze_trigger, last_analyzed = ?heap.last_analyzed, "PostgreSQL autoanalyze has not analyzed a relation past twice its trigger");
        }
    }
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
