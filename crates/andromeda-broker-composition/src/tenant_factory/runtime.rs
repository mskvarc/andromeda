//! Runtime resources owned by one tenant and their shutdown order.

use crate::tenant_factory::{background_pacer::BackgroundPacer, periodic_due::PeriodicDue, storage::TenantContextWorkers, tasks::TaskSupervision};
use andromeda_delivery::{dispatch::TransportDispatcher, mqtt::pool::MqttTransportPool};
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_tenancy::state_factory::{RuntimeActivity, TenantRuntime};
use async_trait::async_trait;
use sqlx::PgPool;
use std::{sync::Arc, time::SystemTime};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info};

/// Everything one tenant's runtime holds open.
pub struct BrokerTenantRuntime {
    context_workers: TenantContextWorkers,
    background_handles: Vec<JoinHandle<()>>,
    transport: Arc<TransportDispatcher>,
    pool: PgPool,
    health_shutdown: Option<watch::Sender<bool>>,
    task_supervisor: TaskSupervisor,
    /// Paces the tenant's sweeps; told of every use of the runtime.
    pacer: Arc<BackgroundPacer>,
    /// When the tenant's periodic notification sweeps are next due.
    periodic_due: Arc<PeriodicDue>,
}

impl BrokerTenantRuntime {
    pub(super) fn new(
        context_workers: TenantContextWorkers,
        supervision: TaskSupervision,
        transport: Arc<TransportDispatcher>,
        pool: PgPool,
        pacer: Arc<BackgroundPacer>,
        periodic_due: Arc<PeriodicDue>,
    ) -> BrokerTenantRuntime {
        let TaskSupervision {
            handles,
            health_shutdown,
            task_supervisor,
        } = supervision;
        BrokerTenantRuntime {
            context_workers,
            background_handles: handles,
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
}

#[async_trait]
impl TenantRuntime for BrokerTenantRuntime {
    async fn shutdown(self) {
        if let Some(shutdown) = self.health_shutdown {
            let _ = shutdown.send(true);
        }
        self.task_supervisor.shutdown().await;
        stop_background_tasks(self.background_handles).await;
        info!(event = "tenant_background_tasks_stopped");
        self.transport.shutdown().await;
        self.context_workers.shutdown().await;
        info!(event = "tenant_context_hits_flushed");
        self.pool.close().await;
        info!(event = "tenant_pool_closed");
    }

    fn activity(&self) -> Arc<dyn RuntimeActivity> {
        Arc::clone(&self.pacer) as Arc<dyn RuntimeActivity> // clone: the cache reports use to the pacer the tenant's tasks consult.
    }

    fn next_periodic_due(&self) -> Option<SystemTime> {
        self.periodic_due.earliest()
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
        let task_shutdown = source.find("self.task_supervisor.shutdown().await").ok_or("task shutdown call missing")?;
        let background_shutdown = source
            .find("stop_background_tasks(self.background_handles).await")
            .ok_or("background task shutdown call missing")?;
        let transport_shutdown = source.find("self.transport.shutdown().await").ok_or("transport shutdown call missing")?;
        let pool_shutdown = source.find("self.pool.close().await").ok_or("pool shutdown call missing")?;
        assert!(task_shutdown < background_shutdown);
        assert!(background_shutdown < transport_shutdown);
        assert!(transport_shutdown < pool_shutdown);
        Ok(())
    }
}
