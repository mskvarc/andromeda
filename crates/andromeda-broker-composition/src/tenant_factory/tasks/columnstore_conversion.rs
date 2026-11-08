//! Registration of the columnstore conversion sweep.
//!
//! On an edition with a columnstore, every tenant converts its aged history
//! chunks itself, beside retention, instead of relying on a `TimescaleDB`
//! background job that a server with more databases than background workers
//! never runs. The first sweep runs when the tenant is opened, so a tenant the
//! wake loop loaded for a chunk past the horizon converts it while it stays
//! loaded.
//! A sweep stops between chunks once its tenant is shutting down. After the
//! chunks it then rebuilds the rowstore indexes converted chunks left at full
//! size.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError, tasks::temporal_sweep_failure::classify_sweep_failure};
use andromeda_configuration::schema::temporal::storage::ColumnstoreConfig;
use andromeda_postgresql_temporal_store::{
    columnstore_capability::ColumnstoreSupport,
    columnstore_conversion::{ChunkConversion, ColumnstoreHorizon, ConversionCandidates},
    repo::TemporalRepository,
    rowstore_reclaim::RowstoreReclaim,
};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use chrono::Utc;
use std::{future::Future, sync::Arc};
use tokio_util::sync::CancellationToken;

/// What one sweep did with the chunks it found due.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ConversionTally {
    /// Chunks converted.
    converted: usize,
    /// Chunks kept in the rowstore because converting them would not pay.
    kept: usize,
    /// Chunks another replica held or had converted.
    passed_over: usize,
    /// Chunks whose conversion was rolled back for the next sweep.
    deferred: usize,
    /// Chunks left unattempted because the tenant began shutting down.
    unattempted: usize,
}

impl ConversionTally {
    /// Whether the sweep found anything to do.
    const fn is_empty(self) -> bool {
        self.converted == 0 && self.kept == 0 && self.passed_over == 0 && self.deferred == 0 && self.unattempted == 0
    }
}

/// Register the conversion sweep when the tenant's edition has a columnstore.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the configured cadence
/// names no usable schedule, or when the supervisor refuses the task.
pub(super) fn register_columnstore_conversion(
    supervisor: &TaskSupervisor,
    repository: Arc<TemporalRepository>,
    horizon: ColumnstoreHorizon,
    config: &ColumnstoreConfig,
) -> Result<(), TenantAssemblyError> {
    match repository.columnstore() {
        ColumnstoreSupport::Absent => return Ok(()),
        ColumnstoreSupport::Present => {}
    }
    let cadence = config.sweep_interval.duration();
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("temporal-columnstore-conversion").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(cadence)),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let repository = Arc::clone(&repository);
        async move {
            let candidates = repository
                .columnstore_conversion_candidates(horizon, Utc::now())
                .await
                .map_err(classify_sweep_failure)?;
            let ConversionCandidates::Due { cutoff, chunks } = candidates else {
                return Ok(());
            };
            let tally = convert_each(&chunks, execution.cancellation(), |chunk| repository.convert_chunk_to_columnstore(chunk))
                .await
                .map_err(classify_sweep_failure)?;
            if !tally.is_empty() {
                tracing::info!(
                    %cutoff,
                    converted = tally.converted,
                    kept = tally.kept,
                    passed_over = tally.passed_over,
                    deferred = tally.deferred,
                    unattempted = tally.unattempted,
                    event = "temporal_columnstore_converted",
                    "temporal history chunks past the columnstore horizon converted"
                );
            }
            if execution.cancellation().is_cancelled() {
                return Ok(());
            }
            let reclaim = repository.reclaim_converted_rowstores().await.map_err(classify_sweep_failure)?;
            if reclaim != RowstoreReclaim::default() {
                tracing::info!(
                    rebuilt = reclaim.rebuilt,
                    deferred = reclaim.deferred,
                    event = "temporal_rowstore_indexes_reclaimed",
                    "rowstore indexes of converted history chunks rebuilt"
                );
            }
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// Convert `chunks` in order with `convert`, stopping before the next chunk
/// once `cancellation` is requested.
async fn convert_each<'c, C, F, Fut, E>(chunks: &'c [C], cancellation: &CancellationToken, mut convert: F) -> Result<ConversionTally, E>
where
    C: Sync,
    F: FnMut(&'c C) -> Fut + Send,
    Fut: Future<Output = Result<ChunkConversion, E>> + Send,
{
    let mut tally = ConversionTally::default();
    for (index, chunk) in chunks.iter().enumerate() {
        if cancellation.is_cancelled() {
            tally.unattempted = chunks.len() - index;
            break;
        }
        match convert(chunk).await? {
            ChunkConversion::Converted => tally.converted += 1,
            ChunkConversion::KeptInRowstore => tally.kept += 1,
            ChunkConversion::PassedOver => tally.passed_over += 1,
            ChunkConversion::Deferred => tally.deferred += 1,
        }
    }
    Ok(tally)
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::columnstore_conversion::{ConversionTally, convert_each};
    use andromeda_postgresql_temporal_store::columnstore_conversion::ChunkConversion;
    use pretty_assertions::assert_eq;
    use std::{convert::Infallible, sync::Mutex};
    use tokio_util::sync::CancellationToken;

    /// Chunk names standing in for the chunk relations a sweep found due.
    fn chunks(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| format!("_timescaledb_internal.{name}")).collect()
    }

    #[tokio::test]
    async fn every_due_chunk_is_converted_in_order_and_its_outcome_counted() {
        let due = chunks(&["a", "b", "c", "d"]);
        let outcomes = [
            ChunkConversion::Converted,
            ChunkConversion::KeptInRowstore,
            ChunkConversion::PassedOver,
            ChunkConversion::Deferred,
        ];
        let visited = Mutex::new(Vec::new());
        let tally = convert_each(&due, &CancellationToken::new(), |chunk| {
            let mut seen = visited.lock().expect("the record is not poisoned");
            seen.push(chunk.clone());
            let outcome = outcomes[seen.len() - 1];
            async move { Ok::<_, Infallible>(outcome) }
        })
        .await;
        assert_eq!(
            tally,
            Ok(ConversionTally {
                converted: 1,
                kept: 1,
                passed_over: 1,
                deferred: 1,
                unattempted: 0,
            })
        );
        assert_eq!(visited.into_inner().expect("the record is not poisoned"), due);
    }

    #[tokio::test]
    async fn a_sweep_stops_before_the_next_chunk_once_cancelled() {
        let due = chunks(&["a", "b", "c"]);
        let cancellation = CancellationToken::new();
        let tally = convert_each(&due, &cancellation, |_chunk| {
            cancellation.cancel();
            async { Ok::<_, Infallible>(ChunkConversion::Converted) }
        })
        .await;
        assert_eq!(
            tally,
            Ok(ConversionTally {
                converted: 1,
                kept: 0,
                passed_over: 0,
                deferred: 0,
                unattempted: 2,
            })
        );
    }

    #[tokio::test]
    async fn a_failed_conversion_ends_the_sweep_with_its_error() {
        let due = chunks(&["a", "b"]);
        let attempts = Mutex::new(0);
        let tally = convert_each(&due, &CancellationToken::new(), |_chunk| {
            *attempts.lock().expect("the count is not poisoned") += 1;
            async { Err::<ChunkConversion, &str>("refused") }
        })
        .await;
        assert_eq!(tally, Err("refused"));
        assert_eq!(attempts.into_inner().expect("the count is not poisoned"), 1);
    }
}
