//! Merging locally held entities with those a context source returned
//! (clause 5.7.2).

use crate::query::EntityQueryService;
use andromeda_federation::{entity_result::merge_query_results, outcome::ForwardedEntities};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use iri_rs::IriBuf;

impl EntityQueryService {
    /// Merge local query results with forwarded context source results.
    ///
    /// The merge runs on the model, by expanded name. Clause 5.7.2 applies
    /// `attrs` after the merge, because a context source returns every
    /// attribute it holds: `attrs` holds the expanded names the query selects,
    /// matched exactly, and an entity left with none of them is no answer
    /// (clause 5.7.2.4). An empty `attrs` selects every attribute.
    #[must_use]
    pub fn merge_with_forwarded(local_entities: Vec<ReceivedState>, forwarded: Vec<ForwardedEntities>, attrs: &[IriBuf]) -> Vec<ReceivedState> {
        let mut result = merge_query_results(local_entities, forwarded);
        if !attrs.is_empty() {
            for version in &mut result {
                version
                    .state
                    .attributes_mut()
                    .retain(|name, _| attrs.iter().any(|wanted| wanted.as_str() == name.as_str()));
            }
            result.retain(|version| !version.state.attributes().is_empty());
        }
        result
    }
}
