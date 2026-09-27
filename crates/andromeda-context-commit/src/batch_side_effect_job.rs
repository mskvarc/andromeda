//! Durable work produced by the opt-in asynchronous batch path.
//!
//! Only the live branch defers side effects: a request scoped to a Snapshot
//! runs them inline, so a job always finishes a live-branch write and names
//! no branch.

use crate::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;
use std::sync::Arc;

/// Temporal writes and change events deferred by one live-branch entity write.
#[derive(Debug)]
pub struct BatchSideEffectJob {
    /// Entities whose observations still have to reach Temporal Evolution.
    pub temporal_entities: Vec<(Arc<Uri>, Arc<Value>)>,
    /// Changes whose notifications the queuing commit did not record as
    /// durable deliveries, and which whichever broker drains the job therefore
    /// still owes its subscribers.
    ///
    /// A broker that records durable deliveries at commit leaves this empty:
    /// its notifications are already queued, and carrying the changes as well
    /// would notify them a second time when a live-delivery broker drains the
    /// job.
    pub change_events: Vec<ChangeEvent>,
}

#[cfg(test)]
mod tests {
    use crate::batch_side_effect_job::BatchSideEffectJob;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn a_job_keeps_shared_temporal_payloads_without_copying_them() {
        let payload = Arc::new(json!([{"@id": "urn:ngsi-ld:A"}]));
        let job = BatchSideEffectJob {
            temporal_entities: vec![(Arc::new("urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI")), Arc::clone(&payload))],
            change_events: Vec::new(),
        };
        assert!(Arc::ptr_eq(&payload, &job.temporal_entities[0].1));
    }
}
