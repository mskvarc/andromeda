//! The compiled form of an NGSI-LD geo query (clause 4.10).

use crate::queried_entity::located_entity::LocatedEntity;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, planar::PlanarGeometry, relation::matches};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// A compiled geo-spatial query, per NGSI-LD 4.10.
///
/// The `geometry` and `coordinates` parameters arrive as two separate strings
/// and denote one `GeoJSON` geometry between them, so they are joined and parsed
/// during compilation. Everything downstream receives a geometry that is known
/// to be well formed, and a malformed one is a 400 from the request rather than
/// a failure part-way through building SQL.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoQuery {
    /// The spatial relationship to evaluate.
    pub georel: GeoRel,
    /// The reference geometry the relation is evaluated against.
    pub geometry: Geometry,
    /// The `GeoProperty` to query against, named by its fully expanded IRI.
    ///
    /// Clause 4.10 lets `geoproperty` name any `GeoProperty` of the entity, and a
    /// `GeoProperty` is an attribute, so the target is the attribute name that
    /// keys it (clause 5.2.4) rather than a bare IRI. Holding it as such is
    /// what lets the target be looked up in the attribute-name dictionary
    /// without re-parsing it at every use.
    pub geoproperty: AttributeName,
}

impl GeoQuery {
    /// Whether an entity satisfies this geo-query (clause 4.10).
    ///
    /// Where the target `GeoProperty` has several `datasetId` instances, one of
    /// them meeting the restriction is enough, and an entity conveying no
    /// instance of it matches nothing. This is the same reading the SQL
    /// rendering gives the clause with its correlated `EXISTS`, so a query and
    /// the equivalent subscription cannot disagree about a multi-instance
    /// `GeoProperty`. The reference geometry is projected once, and only when
    /// the entity has an instance to compare with it.
    pub fn matches_entity<'a, E: LocatedEntity<'a>>(&self, entity: E) -> bool {
        let mut subjects = entity.geo_property_geometries(&self.geoproperty).peekable();
        if subjects.peek().is_none() {
            return false;
        }
        let reference = self.geometry.planar();
        subjects.any(|subject| matches(&self.georel, &PlanarGeometry::project(&subject), &reference))
    }
}

#[cfg(test)]
mod tests {
    use crate::{geo::query::GeoQuery, queried_entity::located_entity::LocatedEntity};
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, geo_rel::GeoRel, geometry::Geometry, position::Position};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::json;
    use std::borrow::Cow;

    const CUSTOM_GEOPROPERTY: &str = "https://example.com/observationPoint";

    /// An entity holding the instances of its `GeoProperty` attributes as
    /// points, in order.
    struct PlacedEntity(Vec<(AttributeName, GeometryForm)>);

    impl<'a> LocatedEntity<'a> for &'a PlacedEntity {
        fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>> {
            self.0.iter().filter(move |(name, _)| name == attribute).map(|(_, form)| Cow::Borrowed(form))
        }
    }

    /// An entity whose `geoproperty` carries one point instance per coordinate
    /// pair.
    fn entity_with_instances(geoproperty: &str, points: &[[f64; 2]]) -> PlacedEntity {
        let name: AttributeName = geoproperty.parse().expect("valid attribute name");
        PlacedEntity(
            points
                .iter()
                .map(|[longitude, latitude]| {
                    let position = Position::try_new(*longitude, *latitude, None).expect("a test position lies on the CRS84 axes");
                    (name.clone(), GeometryForm::Point(position)) // clone: every instance is keyed by the same name
                })
                .collect(),
        )
    }

    /// A `within` query over a small box around `(13.4, 52.5)`.
    fn within_berlin(geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: serde_json::from_value::<Geometry>(json!({
                "type": "Polygon",
                "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
            }))
            .expect("polygon parses"),
            geoproperty: geoproperty.parse().expect("valid attribute name"),
        }
    }

    /// Clause 4.10: where a `GeoProperty` has several `datasetId` instances, one
    /// of them meeting the restriction is enough, whichever it is.
    #[test]
    fn any_instance_meeting_the_restriction_is_enough() {
        let query = within_berlin(CUSTOM_GEOPROPERTY);
        assert!(query.matches_entity(&entity_with_instances(CUSTOM_GEOPROPERTY, &[[0.0, 0.0], [13.4, 52.5]])));
        assert!(query.matches_entity(&entity_with_instances(CUSTOM_GEOPROPERTY, &[[13.4, 52.5], [0.0, 0.0]])));
    }

    #[test]
    fn an_entity_no_instance_of_which_meets_the_restriction_does_not_match() {
        let entity = entity_with_instances(CUSTOM_GEOPROPERTY, &[[0.0, 0.0], [1.0, 1.0]]);
        assert!(!within_berlin(CUSTOM_GEOPROPERTY).matches_entity(&entity));
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching, whatever the relation — even `disjoint`, which an absent
    /// geometry would otherwise trivially satisfy.
    #[test]
    fn an_entity_conveying_no_instance_of_the_target_geoproperty_does_not_match() {
        let entity = entity_with_instances("https://example.com/somethingElse", &[[13.4, 52.5]]);
        assert!(!within_berlin(CUSTOM_GEOPROPERTY).matches_entity(&entity));
        let disjoint = GeoQuery {
            georel: GeoRel::Disjoint,
            ..within_berlin(CUSTOM_GEOPROPERTY)
        };
        assert!(!disjoint.matches_entity(&entity));
    }
}
