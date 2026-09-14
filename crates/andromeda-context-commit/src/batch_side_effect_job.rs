//! Durable work produced by the opt-in asynchronous batch path.

use crate::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;
use std::sync::Arc;

/// Temporal writes and change events deferred by one entity write.
#[derive(Debug)]
pub struct BatchSideEffectJob {
    /// Context branch whose consequences the job must finish.
    pub target: ContextTarget,
    /// Entities whose observations still have to reach Temporal Evolution.
    pub temporal_entities: Vec<(Arc<Uri>, Arc<Value>)>,
    /// Change events still to be emitted.
    pub change_events: Vec<ChangeEvent>,
}

#[cfg(test)]
mod tests {
    use crate::batch_side_effect_job::BatchSideEffectJob;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn a_job_keeps_shared_temporal_payloads_without_copying_them() {
        let payload = Arc::new(json!([{"@id": "urn:ngsi-ld:A"}]));
        let job = BatchSideEffectJob {
            target: ContextTarget::Live,
            temporal_entities: vec![(Arc::new("urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI")), Arc::clone(&payload))],
            change_events: Vec::new(),
        };
        assert!(Arc::ptr_eq(&payload, &job.temporal_entities[0].1));
    }
}
