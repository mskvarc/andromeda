//! Compacting the histories one temporal read gathered under its request
//! context (clause 5.5.7).
//!
//! One read gathers many entities, each held by one or several sources, in the
//! context-independent form; the request context is processed once, the first
//! time anything is compacted, and every entity of the read is compacted
//! against it. What each holder contributed carries its clause 4.8 times, which
//! the merge reads (clause 4.5.5.3); whether the response writes them is
//! decided here, once, the same way for every holder.

use crate::{error::TemporalError, service::TemporalService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_model::temporal::{temporal_body::TemporalBody, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

impl TemporalService {
    /// The histories `histories` as the compact Temporal Evolutions they are
    /// under `context`, in the same order, their instances written with their
    /// `createdAt` and `modifiedAt` as `system_times` says.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the request context does not
    /// resolve or process, and [`TemporalError::Internal`] when an assembled
    /// entity does not compact.
    pub async fn compact_histories(
        &self,
        histories: Vec<TemporalHistory>,
        system_times: SysAttrsInclusion,
        context: &ContextSpec,
    ) -> Result<Vec<TemporalBody>, TemporalError> {
        let compactor = self.engine.compactor(context);
        let compactor = EntityCompactor::new(&compactor);
        let mut bodies = Vec::with_capacity(histories.len());
        for history in histories {
            bodies.push(compact_history(&compactor, history, system_times).await?);
        }
        Ok(bodies)
    }
}

/// The compact Temporal Evolution `history` is under the request context
/// `compactor` serves, written with the system times `system_times` keeps.
///
/// # Errors
///
/// As [`TemporalService::compact_histories`].
pub(crate) async fn compact_history(
    compactor: &EntityCompactor<'_>,
    history: TemporalHistory,
    system_times: SysAttrsInclusion,
) -> Result<TemporalBody, TemporalError> {
    match history {
        TemporalHistory::Instances(document) => Ok(TemporalBody::Evolution(compactor.temporal(document, system_times).await?)),
        // The names an aggregate carries are the request context's already.
        TemporalHistory::Aggregates(entity) => Ok(TemporalBody::Aggregated(entity)),
    }
}
