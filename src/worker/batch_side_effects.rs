use crate::{
    domain::notification::ChangeEvent,
    repository::temporal::repo::TemporalRepository,
    service::{entity::BatchSideEffectJob, temporal_recorder::TemporalRecorder},
};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Background worker for deferred batch side effects.
pub struct BatchSideEffectWorker {
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    receiver: mpsc::Receiver<BatchSideEffectJob>,
    temporal_recorder: TemporalRecorder,
}

impl BatchSideEffectWorker {
    /// Create a new batch side-effect worker.
    pub const fn new(
        temporal_repo: Arc<TemporalRepository>,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        receiver: mpsc::Receiver<BatchSideEffectJob>,
    ) -> BatchSideEffectWorker {
        BatchSideEffectWorker {
            change_tx,
            receiver,
            temporal_recorder: TemporalRecorder::new(temporal_repo),
        }
    }

    /// Run the worker loop until all senders are dropped.
    pub async fn run(mut self) {
        while let Some(job) = self.receiver.recv().await {
            self.handle_job(job).await;
        }
    }

    async fn handle_job(&self, job: BatchSideEffectJob) {
        let BatchSideEffectJob {
            temporal_entities,
            change_events,
        } = job;

        if let Err(error) = self.temporal_recorder.record_entities_batch(&temporal_entities).await {
            tracing::warn!(error = %error, event = "async_temporal_batch_recording_failed");
        }

        if let Some(change_tx) = &self.change_tx {
            for event in change_events {
                let _ = change_tx.send(event);
            }
        }
    }
}
