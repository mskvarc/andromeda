//! Whether an entity held in memory passes the value, geo and scope filters
//! that select it.

use crate::{
    geo_query::geo_query_matches,
    q::filter_run::filter_holds,
    queried_entity::{attribute_reading::QueriedEntity, linked_entities::LinkedEntities, located_entity::LocatedEntity, scoped_entity::ScopedEntity},
    scope_query::scope_query_matches,
};
use andromeda_ngsi_ld_query::entity_filters::EntityFilters;

/// Whether `entity` passes every filter `filters` carries: `q` (clause 4.9),
/// the entities its `rel{…}` hops cross into read from `linked`, the
/// geo-query (clause 4.10) and the scope query (clause 4.19).
///
/// A filter the query or Subscription does not carry passes every entity, so
/// with none every entity is admitted (clause 5.8.6).
pub fn entity_filters_admit<'a, E, L>(filters: EntityFilters<'_>, entity: E, linked: L) -> bool
where
    E: QueriedEntity<'a> + LocatedEntity<'a> + ScopedEntity<'a>,
    L: LinkedEntities<'a>,
{
    filters.q.is_none_or(|q| filter_holds(q.reading(), entity, linked))
        && filters.geo.is_none_or(|geo| geo_query_matches(geo, entity))
        && filters.scope.is_none_or(|scope| scope_query_matches(scope, entity.scopes()))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_filters::entity_filters_admit,
        queried_entity::{
            linked_entities::NoLinkedEntities,
            placed_fixture::{LOCATION, PlacedFixture},
        },
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{
        entity_filters::EntityFilters,
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse},
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
    };
    use serde_json::json;

    /// The `q` every case but the first carries, which the entity meets.
    const N_IS_TEN: &str = "\"https://example.com/n\"==10";

    /// The entity `x`: `n` is 10, its `location` the point (13.4, 52.5) and
    /// its scope `/A/B`.
    fn x() -> PlacedFixture {
        PlacedFixture::new(
            &json!({
                "@id": "urn:ngsi-ld:T:x",
                "@type": ["https://example.com/T"],
                "https://example.com/n": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]}]
            }),
            Some([13.4, 52.5]),
            &["/A/B"],
        )
    }

    /// Within 1000 metres of the point (`longitude`, `latitude`).
    fn near(longitude: f64, latitude: f64) -> GeoQuery {
        GeoQuery {
            georel: "near;maxDistance==1000".parse::<GeoRel>().expect("a clause 4.10 relation"),
            geometry: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [longitude, latitude]})).expect("a point"),
            geoproperty: LOCATION.parse().expect("an attribute name"),
        }
    }

    /// Whether `x` passes the filters given.
    fn admits_x(q: Option<&str>, geo: Option<&GeoQuery>, scope: Option<&str>) -> bool {
        let q = q.map(|raw| CompiledFilter::new(parse(raw).expect("q parses"), LanguageMatching::TagEquality));
        let scope = scope.map(|raw| ScopeQuery::parse(raw).expect("scopeQ parses"));
        let filters = EntityFilters {
            q: q.as_ref(),
            geo,
            scope: scope.as_ref(),
        };
        entity_filters_admit(filters, &x(), NoLinkedEntities)
    }

    #[test]
    fn no_filter_admits_the_entity() {
        assert!(admits_x(None, None, None));
    }

    #[test]
    fn a_q_the_entity_meets_admits_it() {
        assert!(admits_x(Some(N_IS_TEN), None, None));
    }

    /// (2.35, 48.85) is some 880 km from (13.4, 52.5).
    #[test]
    fn a_geo_query_the_entity_fails_refuses_it_though_q_holds() {
        assert!(!admits_x(Some(N_IS_TEN), Some(&near(2.35, 48.85)), None));
    }

    #[test]
    fn a_scope_query_the_entity_fails_refuses_it_though_q_holds() {
        assert!(!admits_x(Some(N_IS_TEN), None, Some("/C/#")));
    }

    #[test]
    fn an_entity_meeting_q_the_geo_query_and_the_scope_query_is_admitted() {
        assert!(admits_x(Some(N_IS_TEN), Some(&near(13.4, 52.5)), Some("/A/#")));
    }
}
