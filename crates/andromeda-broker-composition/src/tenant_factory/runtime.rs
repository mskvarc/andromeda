//! Runtime resources owned by one tenant and their shutdown order.

use crate::{
    context_hit_flush::ContextHitFlush,
    tenant_factory::{
        background_pacer::{BackgroundPacer, TenantObligations},
        due_work_check::TenantDueWork,
        periodic_due::PeriodicDue,
        tasks::TaskSupervision,
    },
};
use andromeda_delivery::{dispatch::TransportDispatcher, mqtt::pool::MqttTransportPool};
use andromeda_notification::pending_change::PendingCommits;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_tenancy::{
    due_work::DueWorkSource,
    state_factory::{RuntimeActivity, TenantRuntime},
    teardown_bound::TeardownBound,
};
use async_trait::async_trait;
use sqlx::PgPool;
use std::{sync::Arc, time::SystemTime};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::task::TaskTracker;
use tracing::{error, info};

/// Everything one tenant's runtime holds open.
pub(crate) struct BrokerTenantRuntime {
    /// Context hit accounting, drained after the transport stops so the last
    /// hits land before the pool closes; `None` when the engine counts none.
    hit_flush: Option<Arc<ContextHitFlush>>,
    /// Unsupervised tasks, aborted and joined after the hand-offs finish.
    background_handles: Vec<JoinHandle<()>>,
    /// The hand-offs of committed changes to notification evaluation.
    handoffs: Vec<TaskTracker>,
    /// The committed changes handed to live evaluation and not yet evaluated.
    commits: Arc<PendingCommits>,
    /// The tenant's notification transports, shut down once no task can send.
    transport: Arc<TransportDispatcher>,
    /// The tenant's serving pool, closed last.
    pool: PgPool,
    /// Stops the MQTT health probe; `None` in every tenant but the unnamed
    /// default.
    health_shutdown: Option<watch::Sender<bool>>,
    /// Supervisor of the tenant's registered tasks, shut down first.
    task_supervisor: TaskSupervisor,
    /// Paces the tenant's sweeps; told of every use of the runtime.
    pacer: Arc<BackgroundPacer>,
    /// When the tenant's periodic notification sweeps are next due.
    periodic_due: Arc<PeriodicDue>,
}

impl BrokerTenantRuntime {
    /// Assemble the runtime from the tenant's started workers and the
    /// resources they share.
    pub(super) fn new(
        hit_flush: Option<Arc<ContextHitFlush>>,
        supervision: TaskSupervision,
        transport: Arc<TransportDispatcher>,
        pool: PgPool,
        pacer: Arc<BackgroundPacer>,
        periodic_due: Arc<PeriodicDue>,
    ) -> BrokerTenantRuntime {
        let TaskSupervision {
            handles,
            handoffs,
            commits,
            health_shutdown,
            task_supervisor,
        } = supervision;
        BrokerTenantRuntime {
            hit_flush,
            background_handles: handles,
            handoffs,
            commits,
            transport,
            pool,
            health_shutdown,
            task_supervisor,
            pacer,
            periodic_due,
        }
    }

    /// Return the tenant's shared MQTT connection pool.
    pub(super) fn mqtt_pool(&self) -> &Arc<MqttTransportPool> {
        self.transport.mqtt_pool()
    }

    /// The supervisor of the tenant's background tasks.
    pub(crate) const fn task_supervisor(&self) -> &TaskSupervisor {
        &self.task_supervisor
    }

    /// The tenant's serving pool.
    pub(super) const fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl TenantRuntime for BrokerTenantRuntime {
    async fn shutdown(self, bound: TeardownBound) {
        if let Some(shutdown) = self.health_shutdown {
            let _ = shutdown.send(true);
        }
        stop_supervised_tasks(&self.task_supervisor, bound).await;
        wait_for_handoffs(&self.handoffs).await;
        stop_background_tasks(self.background_handles).await;
        info!(event = "tenant_background_tasks_stopped");
        self.transport.shutdown().await;
        if let Some(flush) = &self.hit_flush {
            flush.drain().await;
        }
        info!(event = "tenant_context_hits_flushed");
        self.pool.close().await;
        info!(event = "tenant_pool_closed");
    }

    fn activity(&self) -> Arc<dyn RuntimeActivity> {
        Arc::clone(&self.pacer) as Arc<dyn RuntimeActivity>
    }

    fn next_periodic_due(&self) -> Option<SystemTime> {
        self.periodic_due.earliest()
    }

    fn undelivered_changes(&self) -> usize {
        self.commits.count()
    }

    fn due_work(&self) -> Arc<dyn DueWorkSource> {
        Arc::new(TenantDueWork::new(Arc::new(TenantObligations::new(&self.pool)), Arc::clone(&self.commits)))
    }
}

/// Stop the tenant's supervised tasks, each within its grace period and by
/// the deadline when the broker is stopping.
async fn stop_supervised_tasks(supervisor: &TaskSupervisor, bound: TeardownBound) {
    match bound {
        TeardownBound::GracePeriods => supervisor.shutdown().await,
        TeardownBound::Deadline(deadline) => supervisor.shutdown_by(deadline).await,
    }
}

/// Wait until no hand-off of committed changes to notification evaluation runs.
///
/// The notification workers have stopped by now, and with them the readers of
/// the queues, so a hand-off still waiting for room ends at its next send.
async fn wait_for_handoffs(handoffs: &[TaskTracker]) {
    for tracker in handoffs {
        tracker.close();
        tracker.wait().await;
    }
}

/// Cancel tenant background tasks and wait until every task has stopped.
async fn stop_background_tasks(handles: Vec<JoinHandle<()>>) {
    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        match handle.await {
            Ok(()) => {}
            Err(join_error) if join_error.is_cancelled() => {}
            Err(join_error) => error!(error = %join_error, event = "tenant_worker_join_failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shutdown_awaits_tasks_before_transport_and_pool_closure() -> Result<(), &'static str> {
        let source = include_str!("runtime.rs");
        let task_shutdown = source
            .find("stop_supervised_tasks(&self.task_supervisor, bound).await")
            .ok_or("task shutdown call missing")?;
        let background_shutdown = source
            .find("stop_background_tasks(self.background_handles).await")
            .ok_or("background task shutdown call missing")?;
        let handoff_wait = source.find("wait_for_handoffs(&self.handoffs).await").ok_or("hand-off wait call missing")?;
        let transport_shutdown = source.find("self.transport.shutdown().await").ok_or("transport shutdown call missing")?;
        let hits_drain = source.find("flush.drain().await").ok_or("hit drain call missing")?;
        let pool_shutdown = source.find("self.pool.close().await").ok_or("pool shutdown call missing")?;
        assert!(task_shutdown < handoff_wait);
        assert!(handoff_wait < background_shutdown);
        assert!(task_shutdown < background_shutdown);
        assert!(background_shutdown < transport_shutdown);
        assert!(transport_shutdown < hits_drain);
        assert!(hits_drain < pool_shutdown);
        Ok(())
    }
}
