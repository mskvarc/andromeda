//! Compacting the histories one temporal read gathered under its request
//! context (clause 5.5.7).
//!
//! One read gathers many entities, each held by one or several sources, in the
//! context-independent form; the request context is processed once, the first
//! time anything is compacted, and every entity of the read is compacted
//! against it.

use crate::{error::TemporalError, service::TemporalService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_model::temporal::{temporal_body::TemporalBody, temporal_history::TemporalHistory};

impl TemporalService {
    /// The histories `histories` as the compact Temporal Evolutions they are
    /// under `context`, in the same order.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the request context does not
    /// resolve or process, and [`TemporalError::Internal`] when an assembled
    /// entity does not compact.
    pub async fn compact_histories(&self, histories: Vec<TemporalHistory>, context: &ContextSpec) -> Result<Vec<TemporalBody>, TemporalError> {
        let compactor = self.engine.compactor(context);
        let compactor = EntityCompactor::new(&compactor);
        let mut bodies = Vec::with_capacity(histories.len());
        for history in histories {
            bodies.push(compact_history(&compactor, history).await?);
        }
        Ok(bodies)
    }
}

/// The compact Temporal Evolution `history` is under the request context
/// `compactor` serves.
///
/// # Errors
///
/// As [`TemporalService::compact_histories`].
pub(crate) async fn compact_history(compactor: &EntityCompactor<'_>, history: TemporalHistory) -> Result<TemporalBody, TemporalError> {
    match history {
        TemporalHistory::Instances(document) => Ok(TemporalBody::Evolution(compactor.temporal(&document).await?)),
        // The names an aggregate carries are the request context's already.
        TemporalHistory::Aggregates(entity) => Ok(TemporalBody::Aggregated(entity)),
    }
}
