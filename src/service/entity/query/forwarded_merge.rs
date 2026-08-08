//! Merging locally held entities with those a context source returned
//! (clause 5.7.2).

use crate::service::{
    csource_forwarding::{entity_result::merge_query_results, outcome::ForwardedEntities},
    entity::query::EntityQueryService,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;

impl EntityQueryService {
    /// Merge local query results with forwarded context source results.
    ///
    /// The merge stays in the typed domain model throughout: forwarded entities
    /// were already parsed to [`Entity`] at the registration boundary, so no
    /// `Value` round trip occurs. Clause 5.7.2 applies `attrs` after the merge,
    /// because a context source returns every attribute it holds.
    pub fn merge_with_forwarded(local_entities: Vec<Entity>, forwarded: Vec<ForwardedEntities>, attrs_filter: Option<&[AttributeName]>) -> Vec<Entity> {
        let mut result = merge_query_results(local_entities, forwarded);
        if let Some(attrs) = attrs_filter {
            for entity in &mut result {
                entity.attrs.retain(|key, _| attrs.iter().any(|wanted| key.ends_with(wanted.as_str())));
            }
            result.retain(|entity| !entity.attrs.is_empty());
        }
        result
    }
}
