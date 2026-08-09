//! Runtime resources owned by one tenant and their shutdown order.

use crate::api::context_hit_flush::ContextHitFlushWorker;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_tenancy::state_factory::TenantRuntime;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info};

/// Everything one tenant's runtime holds open.
pub struct BrokerTenantRuntime {
    context_hit_flush: Option<ContextHitFlushWorker>,
    worker_handles: Vec<JoinHandle<()>>,
    transport: Arc<TransportDispatcher>,
    pool: PgPool,
    health_shutdown: Option<watch::Sender<bool>>,
}

impl BrokerTenantRuntime {
    pub(super) const fn new(
        context_hit_flush: Option<ContextHitFlushWorker>,
        worker_handles: Vec<JoinHandle<()>>,
        transport: Arc<TransportDispatcher>,
        pool: PgPool,
        health_shutdown: Option<watch::Sender<bool>>,
    ) -> BrokerTenantRuntime {
        BrokerTenantRuntime {
            context_hit_flush,
            worker_handles,
            transport,
            pool,
            health_shutdown,
        }
    }
}

#[async_trait]
impl TenantRuntime for BrokerTenantRuntime {
    async fn shutdown(self) {
        if let Some(shutdown) = self.health_shutdown {
            let _ = shutdown.send(true);
        }
        self.transport.shutdown().await;
        stop_workers(self.worker_handles).await;
        info!(event = "tenant_workers_stopped");
        if let Some(worker) = self.context_hit_flush {
            worker.shutdown().await;
        }
        info!(event = "tenant_context_hits_flushed");
        self.pool.close().await;
        info!(event = "tenant_pool_closed");
    }
}

/// Cancel tenant background workers and wait until every task has stopped.
async fn stop_workers(handles: Vec<JoinHandle<()>>) {
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
