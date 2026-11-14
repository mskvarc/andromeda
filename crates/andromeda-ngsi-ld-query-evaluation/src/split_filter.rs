//! The truth of each leaf of a split query's filters over one part of an
//! entity held in memory (clause 5.7.4.4 `splitEntities`).
//!
//! Every leaf is existential over the instances it reads, so a part is judged
//! on its own and the truths of the parts are joined afterwards.

use crate::{
    geo_query::geo_query_matches,
    q::eval::predicate_holds_in,
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::LinkedEntities, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
};
use andromeda_ngsi_ld_query::split_filter::{FilterLeaf, LeafTruths, SplitFilter};

/// The truth of every leaf of `filter` over one part of an entity, read in
/// memory, the entities a Query Term's hops cross into read from `linked`.
pub fn leaf_truths<'a, E, L>(filter: &SplitFilter<'_>, entity: E, linked: L) -> LeafTruths
where
    E: QueriedEntity<'a> + LocatedEntity<'a> + ScopedEntity<'a>,
    L: LinkedEntities<'a>,
{
    let language_matching = filter.filters().language_matching;
    LeafTruths::from_vec(
        filter
            .leaves()
            .map(|leaf| match leaf {
                FilterLeaf::Term(pred) => predicate_holds_in(pred, entity, linked, language_matching),
                FilterLeaf::Geo(geo) => geo_query_matches(geo, entity),
                FilterLeaf::ScopeRequirement(pattern) => entity.scopes().any(|scope| pattern.matches(scope)),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_filters::entity_filters_admit,
        queried_entity::{
            linked_entities::NoLinkedEntities,
            placed_fixture::{LOCATION, PlacedFixture},
        },
        split_filter::leaf_truths,
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{
        entity_filters::EntityFilters,
        entity_selection::EntitySelection,
        filter::parser::parse,
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
        split_filter::SplitFilter,
    };
    use serde_json::{Value, json};

    /// A `within` query over a box around `(13.4, 52.5)`.
    fn within_berlin() -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(json!({
                "type": "Polygon",
                "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
            }))
            .expect("polygon parses"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    fn selection(q: Option<&str>, geo: Option<GeoQuery>, scope: Option<&str>) -> EntitySelection {
        EntitySelection::builder()
            .q_expr(q.map(|raw| parse(raw).expect("q parses")))
            .geo(geo)
            .scope_q(scope.map(|raw| ScopeQuery::parse(raw).expect("scopeQ parses")))
            .build()
    }

    /// One part of an entity: its `speed`, its `location` point and its
    /// scopes.
    fn part(speed: Option<u32>, point: Option<[f64; 2]>, scopes: &[&str]) -> PlacedFixture {
        let mut document = json!({"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/Vehicle"]});
        if let (Some(speed), Value::Object(members)) = (speed, &mut document) {
            members.insert(
                "https://example.com/speed".into(),
                json!([{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": speed}]}]),
            );
        }
        PlacedFixture::new(&document, point, scopes)
    }

    #[test]
    fn in_memory_truths_match_whole_entity_evaluation() {
        let parts = [
            part(Some(60), Some([13.4, 52.5]), &["/Madrid", "/Spain"]),
            part(Some(40), Some([13.4, 52.5]), &["/Madrid"]),
            part(Some(60), Some([0.0, 0.0]), &["/Spain"]),
            part(None, None, &[]),
        ];
        let filters = [
            selection(Some("\"https://example.com/speed\">50"), Some(within_berlin()), Some("(/Madrid;/Spain)")),
            selection(
                Some("\"https://example.com/speed\">50|\"https://example.com/speed\"<45"),
                None,
                Some("/Madrid,/Spain"),
            ),
            selection(Some("\"https://example.com/speed\""), Some(within_berlin()), None),
            selection(None, None, Some("/Spain")),
        ];
        for (filter_index, selection) in filters.iter().enumerate() {
            let filter = SplitFilter::of(selection);
            for (part_index, part) in parts.iter().enumerate() {
                pretty_assertions::assert_eq!(
                    filter.holds(&leaf_truths(&filter, part, NoLinkedEntities)),
                    entity_filters_admit(EntityFilters::of(selection), part, NoLinkedEntities),
                    "filter {filter_index} over part {part_index}"
                );
            }
        }
    }
}
