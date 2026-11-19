//! Merging locally held entities with those a context source returned
//! (clause 5.7.2).

use crate::query::EntityQueryService;
use andromeda_federation::{entity_result::merge_query_results, outcome::ForwardedEntities};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;

impl EntityQueryService {
    /// Merge local query results with forwarded context source results.
    ///
    /// The merge runs on the model, by expanded name, and keeps every member a
    /// holder contributed; the request's `AnswerList` applies its `attrs`,
    /// `pick` or `omit` to the result.
    #[must_use]
    pub fn merge_with_forwarded(local_entities: Vec<ReceivedState>, forwarded: Vec<ForwardedEntities>) -> Vec<ReceivedState> {
        merge_query_results(local_entities, forwarded)
    }
}
