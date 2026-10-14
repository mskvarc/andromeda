//! Matching a geoquery against the geometry a Context Source Registration
//! declares (clauses 5.10.2.4 and 5.11.2.4).
//!
//! A registration's `location`, `observationSpace` and `operationSpace` (Table
//! 5.2.9-1) bound where the source's Entities are, not where one Entity is.
//! Both clauses therefore let the geoquery match "if the location directly
//! matches or if the location possibly contains locations that would match the
//! geoquery". An Entity geometry `E` lying inside the registration geometry `R`
//! can meet each relation only if `R` meets a weaker one:
//!
//! - `within`, `intersects` and `overlaps` need some part of `E`, hence of `R`,
//!   to meet the reference: `R` intersects it;
//! - `contains` and `equals` need the reference inside `E`, hence inside `R`:
//!   `R` contains it;
//! - `disjoint` needs some part of `E`, hence of `R`, outside the reference:
//!   `R` is not within it;
//! - `near` with a `maxDistance` needs some point of `R` that close: the
//!   shortest distance from `R` is within it. A `minDistance` alone bounds
//!   nothing, since `R` may reach arbitrarily far from its nearest point.
//!
//! Each weaker condition holds wherever the relation holds for `R` itself, so
//! a direct match is always kept. Discovery evaluates the condition in the
//! store, and forwarding and registration subscriptions evaluate it in memory;
//! all three read it from here.

use crate::geo::query::GeoQuery;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, planar::PlanarGeometry, relation::matches};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBSERVATION_SPACE, OPERATION_SPACE};

/// Which of a registration's geometries a geoquery is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationGeometry {
    /// `location`: the default, and the target of any `geoproperty` naming
    /// neither of the two spaces.
    Location,
    /// `observationSpace`, when the `geoproperty` names it.
    ObservationSpace,
    /// `operationSpace`, when the `geoproperty` names it.
    OperationSpace,
}

impl RegistrationGeometry {
    /// The registration geometry `geo` names through its `geoproperty`.
    #[must_use]
    pub fn of(geo: &GeoQuery) -> RegistrationGeometry {
        let geoproperty = geo.geoproperty.as_str();
        if geoproperty == OBSERVATION_SPACE.as_str() {
            RegistrationGeometry::ObservationSpace
        } else if geoproperty == OPERATION_SPACE.as_str() {
            RegistrationGeometry::OperationSpace
        } else {
            RegistrationGeometry::Location
        }
    }
}

/// What a registration geometry has to meet against the reference geometry
/// for an Entity inside it to possibly match a geoquery.
#[derive(Debug, Clone, PartialEq)]
pub enum AreaCondition {
    /// The registration geometry stands in this relation to the reference.
    Relation(GeoRel),
    /// The registration geometry is not within the reference.
    NotWithin,
    /// Any registration geometry may hold a match.
    Unconstrained,
}

impl AreaCondition {
    /// The condition a registration geometry has to meet for `geo`.
    #[must_use]
    pub const fn of(geo: &GeoQuery) -> AreaCondition {
        match &geo.georel {
            GeoRel::Within | GeoRel::Intersects | GeoRel::Overlaps => AreaCondition::Relation(GeoRel::Intersects),
            GeoRel::Contains | GeoRel::Equals => AreaCondition::Relation(GeoRel::Contains),
            GeoRel::Disjoint => AreaCondition::NotWithin,
            GeoRel::Near {
                max_distance: Some(max_distance),
                ..
            } => AreaCondition::Relation(GeoRel::Near {
                max_distance: Some(*max_distance),
                min_distance: None,
            }),
            GeoRel::Near { max_distance: None, .. } => AreaCondition::Unconstrained,
        }
    }

    /// Whether `area` meets this condition against `reference`.
    #[must_use]
    pub fn admits(&self, area: &Geometry, reference: &PlanarGeometry) -> bool {
        match self {
            AreaCondition::Relation(georel) => matches(georel, &area.planar(), reference),
            AreaCondition::NotWithin => !matches(&GeoRel::Within, &area.planar(), reference),
            AreaCondition::Unconstrained => true,
        }
    }
}

impl GeoQuery {
    /// Whether a registration declaring `area` for the geometry
    /// [`RegistrationGeometry::of`] names may hold an Entity this geoquery
    /// matches.
    ///
    /// A registration that declares no such geometry may serve any area, so
    /// it is kept.
    #[must_use]
    pub fn admits_registration_area(&self, area: Option<&Geometry>) -> bool {
        area.is_none_or(|area| AreaCondition::of(self).admits(area, &self.geometry.planar()))
    }
}

#[cfg(test)]
mod tests {
    use crate::geo::{
        query::GeoQuery,
        registration_area::{AreaCondition, RegistrationGeometry},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn geometry(value: &serde_json::Value) -> Geometry {
        serde_json::from_value(value.clone()).expect("a valid geometry") // clone: the fixture keeps its JSON
    }

    fn point(longitude: f64, latitude: f64) -> Geometry {
        geometry(&json!({"type": "Point", "coordinates": [longitude, latitude]}))
    }

    /// The square of half-side `half` degrees around a centre.
    fn square(longitude: f64, latitude: f64, half: f64) -> Geometry {
        let (west, east, south, north) = (longitude - half, longitude + half, latitude - half, latitude + half);
        geometry(&json!({
            "type": "Polygon",
            "coordinates": [[[west, south], [east, south], [east, north], [west, north], [west, south]]]
        }))
    }

    fn query(georel: &str, reference: Geometry) -> GeoQuery {
        query_on(georel, reference, LOCATION)
    }

    fn query_on(georel: &str, reference: Geometry, geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel: georel.parse().expect("a valid georel"),
            geometry: reference,
            geoproperty: geoproperty.parse().expect("a valid attribute name"),
        }
    }

    #[test]
    fn a_registration_declaring_no_geometry_is_kept() {
        assert!(query("near;maxDistance==100000", point(-3.7, 40.4)).admits_registration_area(None));
    }

    #[test]
    fn a_near_query_keeps_the_registration_within_its_maximum_distance_only() {
        let near_madrid = query("near;maxDistance==100000", point(-3.7, 40.4));
        assert!(near_madrid.admits_registration_area(Some(&point(-3.7, 40.4))));
        assert!(!near_madrid.admits_registration_area(Some(&point(13.4, 52.5))));
    }

    /// The registration area around Spain reaches within 100 km of Madrid
    /// although its farthest corners do not.
    #[test]
    fn a_near_query_keeps_an_area_whose_nearest_point_is_close_enough() {
        assert!(query("near;maxDistance==100000", point(-3.7, 40.4)).admits_registration_area(Some(&square(-3.7, 40.4, 5.0))));
    }

    /// An area reaches arbitrarily far from its nearest point, so only a
    /// maximum distance bounds it.
    #[test]
    fn a_near_query_with_only_a_minimum_distance_keeps_every_registration() {
        let far_from_madrid = query("near;minDistance==100000", point(-3.7, 40.4));
        assert!(far_from_madrid.admits_registration_area(Some(&point(-3.7, 40.4))));
        assert_eq!(AreaCondition::of(&far_from_madrid), AreaCondition::Unconstrained);
    }

    /// Clause 5.10.2.4: an area covering Spain possibly contains Entities
    /// within a box around Madrid, though it is not within the box itself.
    #[test]
    fn a_within_query_keeps_an_area_that_reaches_into_the_reference() {
        let within_madrid = query("within", square(-3.7, 40.4, 0.5));
        assert!(within_madrid.admits_registration_area(Some(&square(-3.7, 40.4, 5.0))));
        assert!(within_madrid.admits_registration_area(Some(&point(-3.7, 40.4))));
        assert!(!within_madrid.admits_registration_area(Some(&square(13.4, 52.5, 1.0))));
    }

    #[test]
    fn intersects_and_overlaps_keep_an_area_that_meets_the_reference() {
        for georel in ["intersects", "overlaps"] {
            let around_madrid = query(georel, square(-3.7, 40.4, 0.5));
            assert!(around_madrid.admits_registration_area(Some(&square(-3.0, 40.4, 1.0))), "{georel}");
            assert!(!around_madrid.admits_registration_area(Some(&square(13.4, 52.5, 1.0))), "{georel}");
        }
    }

    /// An Entity containing or equal to the reference lies inside the
    /// registration area, so the area contains the reference too.
    #[test]
    fn contains_and_equals_keep_an_area_that_contains_the_reference() {
        for georel in ["contains", "equals"] {
            let at_madrid = query(georel, point(-3.7, 40.4));
            assert!(at_madrid.admits_registration_area(Some(&square(-3.7, 40.4, 5.0))), "{georel}");
            assert!(at_madrid.admits_registration_area(Some(&point(-3.7, 40.4))), "{georel}");
            assert!(!at_madrid.admits_registration_area(Some(&square(13.4, 52.5, 1.0))), "{georel}");
        }
    }

    /// Every Entity inside an area lying wholly within the reference meets
    /// the reference.
    #[test]
    fn a_disjoint_query_drops_only_an_area_wholly_within_the_reference() {
        let outside_spain = query("disjoint", square(-3.7, 40.4, 5.0));
        assert!(!outside_spain.admits_registration_area(Some(&square(-3.7, 40.4, 0.5))));
        assert!(outside_spain.admits_registration_area(Some(&square(-3.7, 44.4, 2.0))));
        assert!(outside_spain.admits_registration_area(Some(&point(13.4, 52.5))));
    }

    #[test]
    fn the_geoproperty_selects_the_registration_geometry() {
        let reference = point(-3.7, 40.4);
        for (geoproperty, selected) in [
            (LOCATION, RegistrationGeometry::Location),
            ("https://uri.etsi.org/ngsi-ld/observationSpace", RegistrationGeometry::ObservationSpace),
            ("https://uri.etsi.org/ngsi-ld/operationSpace", RegistrationGeometry::OperationSpace),
            ("https://example.org/observationPoint", RegistrationGeometry::Location),
        ] {
            assert_eq!(RegistrationGeometry::of(&query_on("within", reference.clone(), geoproperty)), selected); // clone: every row queries the same reference
        }
    }

    #[test]
    fn a_registration_geometry_matching_directly_is_always_kept() {
        let reference = square(-3.7, 40.4, 1.0);
        let inside = point(-3.7, 40.4);
        for georel in [GeoRel::Within, GeoRel::Intersects] {
            let geo = GeoQuery {
                georel,
                geometry: reference.clone(), // clone: every relation queries the same reference
                geoproperty: LOCATION.parse().expect("a valid attribute name"),
            };
            assert!(geo.admits_registration_area(Some(&inside)), "{}", geo.georel);
        }
    }
}
