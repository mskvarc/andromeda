//! Registration of the tasks that keep a tenant's entity spatial index exact,
//! and the wake that brings its background catch-up in.

use crate::{
    blocking_executor::TokioBlockingExecutor,
    tenant_factory::{
        TenantFactoryConfig,
        assembly_invariant::AssemblyInvariant,
        background_pacer::BackgroundPacer,
        error::TenantAssemblyError,
        spatial::location_placement,
        storage::TenantStorage,
        tasks::maintenance::classify_entity_expiry_failure,
    },
};
use andromeda_entity::{
    error::EntityError,
    port::spatial_catch_up_signal::SpatialCatchUpSignal,
    spatial_acceleration::SpatialAcceleration,
    spatial_reconciliation::{RebuildOutcome, rebuild_location_index},
    spatial_watermark::{CatchUpPass, SpatialWatermark},
};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver, WakeHandle, WakeRequest},
};
use std::{future::Future, sync::Arc, time::Duration};

/// The first sleep of an idle catch-up drain, and how long shutdown waits for
/// a catch-up pass.
///
/// A geo query takes in what changed itself and wakes the drain when it finds
/// more, and this process's writes wake it as they commit, so the fallback
/// only recovers a lost wake.
const CATCH_UP_FALLBACK_FLOOR: Duration = Duration::from_secs(1);

/// Wakes one tenant's background spatial catch-up through its drain's wake
/// handle.
struct SpatialCatchUpWaker {
    /// The catch-up drain's wake handle.
    wake: WakeHandle,
}

impl SpatialCatchUpSignal for SpatialCatchUpWaker {
    fn wake(&self) {
        match self.wake.wake() {
            WakeRequest::Scheduled | WakeRequest::Coalesced => (),
            WakeRequest::Closed => tracing::warn!(event = "spatial_catch_up_wake_closed", "the spatial catch-up drain no longer receives wakes"),
        }
    }
}

/// The catch-up signal a tenant's watermark wakes its background catch-up
/// through, wrapping the catch-up drain's wake handle.
pub(in crate::tenant_factory) fn spatial_catch_up_signal(wake: WakeHandle) -> Arc<dyn SpatialCatchUpSignal> {
    Arc::new(SpatialCatchUpWaker { wake })
}

/// Register the tasks that keep the tenant's spatial index exact under the
/// consistency mode it was composed for.
///
/// Every mode with an index rebuilds it periodically, which drops the entries
/// of entities deleted since, and under `commit-synchronous` brings in the
/// writes of another broker process sharing the store. Under `watermark` a
/// wake-driven drain also takes in, in the background, the changes a geo query
/// leaves to it.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured rebuild cadence is
/// rejected or when the supervisor refuses a registration.
pub(super) fn register_spatial_workers(
    supervisor: &TaskSupervisor,
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    catch_up_wake: DrainWakeReceiver,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let cadence = config.entity.spatial_index.rebuild_interval.duration();
    match &storage.spatial {
        SpatialAcceleration::StoreVerified => Ok(()),
        SpatialAcceleration::CommitSynchronous(index) => {
            let index = Arc::clone(index);
            let source = Arc::clone(&storage.entity_ports.spatial_snapshots);
            let placement = Arc::new(location_placement(&storage.tenant)?);
            register_spatial_rebuild(supervisor, cadence, move || {
                let (index, source, placement) = (Arc::clone(&index), Arc::clone(&source), Arc::clone(&placement));
                async move { rebuild_location_index(&index, source.as_ref(), &placement, &TokioBlockingExecutor).await }
            })
        }
        SpatialAcceleration::Watermark(watermark) => {
            register_spatial_catch_up(supervisor, Arc::clone(watermark), cadence, catch_up_wake, pacer)?;
            let watermark = Arc::clone(watermark);
            register_spatial_rebuild(supervisor, cadence, move || {
                let watermark = Arc::clone(&watermark);
                async move { watermark.rebuild().await }
            })
        }
    }
}

/// Register one periodic spatial rebuild pass at `cadence`.
fn register_spatial_rebuild<Rebuild, Pass>(supervisor: &TaskSupervisor, cadence: Duration, rebuild: Rebuild) -> Result<(), TenantAssemblyError>
where
    Rebuild: Fn() -> Pass + Send + Sync + 'static,
    Pass: Future<Output = Result<RebuildOutcome, EntityError>> + Send + 'static,
{
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("spatial-rebuild").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(cadence)),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let pass = rebuild();
        async move {
            let outcome = pass.await.map_err(classify_entity_expiry_failure)?;
            tracing::info!(located_entities = outcome.located_entities, "spatial rebuild completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// Register the wake-driven background catch-up of a `watermark` index.
///
/// A pass after a wake always runs. Without one, a pass runs on the fallback
/// timer, doubling from [`CATCH_UP_FALLBACK_FLOOR`] to the rebuild cadence,
/// when the pacer admits it. A pass that took in a backlog is followed by
/// another at once for what was written while it read.
fn register_spatial_catch_up(
    supervisor: &TaskSupervisor,
    watermark: Arc<SpatialWatermark>,
    cadence: Duration,
    wake: DrainWakeReceiver,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let fallback = DrainFallbackPolicy::new(CATCH_UP_FALLBACK_FLOOR, cadence.max(CATCH_UP_FALLBACK_FLOOR)).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("spatial-catch-up").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, fallback.schedule()),
        ShutdownPolicy::new(CATCH_UP_FALLBACK_FLOOR),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let watermark = Arc::clone(&watermark);
        let pacer = Arc::clone(&pacer);
        async move {
            pacer
                .run_drain_pass(execution.cause(), async move {
                    match watermark.catch_up().await.map_err(classify_entity_expiry_failure)? {
                        CatchUpPass::Settled => Ok(DrainPassOutcome::Idle),
                        CatchUpPass::MoreMayRemain => Ok(DrainPassOutcome::WorkMayRemain),
                    }
                })
                .await
        }
    });
    supervisor.register_drain(task, wake).map_err(|error| AssemblyInvariant::from(error).into())
}
