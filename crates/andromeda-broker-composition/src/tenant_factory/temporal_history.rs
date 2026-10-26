//! Selection of one tenant's temporal history commit mode and read path.

use crate::tenant_factory::{
    background_pacer::BackgroundPacer,
    error::TenantAssemblyError,
    temporal_intent_task::{TemporalIntentRegistration, register_temporal_intent_drain},
};
use andromeda_configuration::schema::temporal::TemporalConfig;
use andromeda_postgresql_atomic_write::{history_mode::TemporalHistoryCommitMode, intent_store::PostgresTemporalIntentStore};
use andromeda_postgresql_temporal_store::{recorder::TemporalRecorder, repo::TemporalRepository};
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_temporal::port::{
    history_reader::TemporalHistoryReader,
    mutation_barrier::TemporalMutationBarrier,
    temporal_intent_application::TemporalIntentApplication,
    temporal_intent_queue::TemporalIntentQueue,
};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};

/// The temporal history capabilities one tenant's commits and reads share.
pub(super) struct TemporalComposition {
    /// How an entity commit records attribute history for this tenant.
    pub(super) history_mode: TemporalHistoryCommitMode,
    /// The ports the request-facing temporal path is answered through.
    pub(super) ports: TemporalPorts,
}

/// The ports the request-facing temporal path reads history through and gates
/// its mutations on.
///
/// The two belong together because queued history couples them: the barrier
/// is what an immediate mutation waits on until the intents a respond-async
/// commit queued have been applied to the history the reader reads.
pub(super) struct TemporalPorts {
    /// The reader temporal reads go through.
    pub(super) reader: Arc<dyn TemporalHistoryReader>,
    /// The barrier an immediate temporal mutation waits on.
    pub(super) mutation_barrier: Arc<dyn TemporalMutationBarrier>,
}

/// Register the intent drain, its mutation barrier bounded by the HTTP
/// `request_timeout`, and select the tenant's history mode.
///
/// The drain always runs, because every respond-async commit queues its
/// live-branch history as intents for it to apply.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured intent batch size,
/// stuck-attempt threshold, retry policy or fallback cadence is out of range,
/// or when the supervisor refuses the drain registration.
pub(super) fn compose_temporal_history(
    pool: &PgPool,
    repository: &Arc<TemporalRepository>,
    config: &TemporalConfig,
    request_timeout: Duration,
    supervisor: &TaskSupervisor,
    pacer: &Arc<BackgroundPacer>,
) -> Result<TemporalComposition, TenantAssemblyError> {
    let intent_store = Arc::new(PostgresTemporalIntentStore::new(pool));
    let TemporalIntentRegistration { publisher, mutation_barrier } = register_temporal_intent_drain(
        config,
        request_timeout,
        supervisor,
        Arc::clone(&intent_store) as Arc<dyn TemporalIntentQueue>, // clone: one store answers both intent ports.
        intent_store as Arc<dyn TemporalIntentApplication>,
        Arc::clone(pacer), // clone: the intent drain consults the tenant's one pacer.
    )?;
    let recorder = Arc::new(TemporalRecorder::new(Arc::clone(repository))); // clone: the recorder and the read path share one repository.
    let history_mode = TemporalHistoryCommitMode::new(recorder, publisher);
    Ok(TemporalComposition {
        history_mode,
        ports: TemporalPorts {
            reader: Arc::clone(repository) as Arc<dyn TemporalHistoryReader>, // clone: the repository answers reads directly.
            mutation_barrier,
        },
    })
}
