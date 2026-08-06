//! Merging locally held entities with those a context source returned
//! (clause 5.7.2).

use crate::service::{
    csource_forwarding::{ForwardedEntities, merge_query_results},
    entity::query::EntityQueryService,
};
use andromeda_ngsi_ld_model::entity::Entity;

impl EntityQueryService {
    /// Merge local query results with forwarded context source results.
    ///
    /// The merge stays in the typed domain model throughout: forwarded entities
    /// were already parsed to [`Entity`] at the registration boundary, so no
    /// `Value` round trip occurs. Clause 5.7.2 applies `attrs` after the merge,
    /// because a context source returns every attribute it holds.
    pub fn merge_with_forwarded(local_entities: Vec<Entity>, forwarded: Vec<ForwardedEntities>, attrs_filter: Option<&str>) -> Vec<Entity> {
        let mut result = merge_query_results(local_entities, forwarded);
        if let Some(attrs) = attrs_filter {
            let requested: Vec<&str> = attrs.split(',').map(str::trim).collect();
            for entity in &mut result {
                entity.attrs.retain(|key, _| requested.iter().any(|wanted| key.ends_with(wanted)));
            }
            result.retain(|entity| !entity.attrs.is_empty());
        }
        result
    }
}
