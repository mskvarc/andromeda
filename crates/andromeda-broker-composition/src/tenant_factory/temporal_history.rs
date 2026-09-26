//! Selection of one tenant's temporal history commit mode and read path.

use crate::tenant_factory::{
    background_pacer::BackgroundPacer,
    error::TenantAssemblyError,
    temporal_intent_task::{TemporalIntentRegistration, register_temporal_intent_drain},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_atomic_write::{history_mode::TemporalHistoryCommitMode, intent_store::PostgresTemporalIntentStore};
use andromeda_postgresql_temporal_store::{intent_sink::PostgresTemporalHistorySink, recorder::TemporalRecorder, repo::TemporalRepository};
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_temporal::{
    config::{TemporalAppendMode, TemporalConfig},
    pending_overlay::PendingTemporalHistoryOverlay,
    port::{
        history_reader::TemporalHistoryReader,
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_intent_queue::TemporalIntentQueue,
    },
};
use sqlx::PgPool;
use std::sync::Arc;

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
/// The two belong together because deferred history couples them: the reader
/// overlays exactly the intents a commit has queued but not yet applied, and
/// the barrier is what an immediate mutation waits on until they have drained.
pub(super) struct TemporalPorts {
    /// The reader temporal reads go through; in deferred mode it overlays the
    /// intents that have been committed but not yet applied.
    pub(super) reader: Arc<dyn TemporalHistoryReader>,
    /// The barrier an immediate temporal mutation waits on.
    pub(super) mutation_barrier: Arc<dyn TemporalMutationBarrier>,
}

/// Register the deferred-history drain and select the tenant's history mode.
///
/// The drain is registered whichever append mode is configured: a tenant moved
/// back to `Native` must still deliver the intents an earlier `Deferred` run
/// committed.
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
    supervisor: &TaskSupervisor,
    live_target: &ContextTarget,
    pacer: &Arc<BackgroundPacer>,
) -> Result<TemporalComposition, TenantAssemblyError> {
    let intent_store = Arc::new(PostgresTemporalIntentStore::new(pool));
    let sink = Arc::new(PostgresTemporalHistorySink::new(Arc::clone(repository))); // clone: the sink and the read path share one repository.
    let TemporalIntentRegistration { publisher, mutation_barrier } = register_temporal_intent_drain(
        live_target.clone(), // clone: the resident deferred-history drain serves the live branch.
        config,
        supervisor,
        Arc::clone(&intent_store) as Arc<dyn TemporalIntentQueue>, // clone: one store answers both intent ports.
        Arc::clone(&intent_store) as Arc<dyn PendingTemporalIntentReader>, // clone: same store, second port.
        sink,
        Arc::clone(pacer), // clone: the intent drain consults the tenant's one pacer.
    )?;
    let (history_mode, reader): (TemporalHistoryCommitMode, Arc<dyn TemporalHistoryReader>) = match config.append_mode {
        TemporalAppendMode::Native => (
            TemporalHistoryCommitMode::Native {
                recorder: Arc::new(TemporalRecorder::new(Arc::clone(repository))), // clone: the recorder and the read path share one repository.
            },
            Arc::clone(repository) as Arc<dyn TemporalHistoryReader>, // clone: the repository answers reads directly.
        ),
        TemporalAppendMode::Deferred => (
            TemporalHistoryCommitMode::Deferred { publisher },
            Arc::new(PendingTemporalHistoryOverlay::new(
                Arc::clone(repository) as Arc<dyn TemporalHistoryReader>, // clone: the overlay reads through the repository.
                intent_store as Arc<dyn PendingTemporalIntentReader>,
            )),
        ),
    };
    Ok(TemporalComposition {
        history_mode,
        ports: TemporalPorts { reader, mutation_barrier },
    })
}
