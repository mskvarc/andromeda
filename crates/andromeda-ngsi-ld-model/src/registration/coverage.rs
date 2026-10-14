//! What a registered Context Source answers for, in space, time and selection (ETSI GS CIM 009 clauses 5.2.9 and 5.2.10).

use crate::{registration::info::RegistrationInfo, time_interval::TimeInterval};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    geo::{query::GeoQuery, registration_area::RegistrationGeometry},
};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// What a context source covers: the entity and attribute selectors it answers
/// for, and the space and time its data spans (ETSI GS CIM 009 clauses 5.2.9
/// and 5.2.10).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationCoverage {
    /// Information blocks describing what this source provides.
    pub information: Vec<RegistrationInfo>,
    /// The `datasetId`s of the attribute instances the source can provide
    /// (clause 4.5.5); `@none` names the default instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// The clause 4.18 scopes the source has entities in. A registration names
    /// places, so the NGSI-LD Null a deleted entity scope is written as is not
    /// one of its values.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub scope: Option<Vec<ScopePath>>,
    /// Location geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub location: Option<Geometry>,
    /// Observation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub observation_space: Option<Geometry>,
    /// Operation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub operation_space: Option<Geometry>,
    /// Interval the source's observations span (clause 5.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub observation_interval: Option<TimeInterval>,
    /// Interval the source's management data spans (clause 5.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub management_interval: Option<TimeInterval>,
}

impl RegistrationCoverage {
    /// The geometry the registration declares for `which`, if it declares one.
    #[must_use]
    pub const fn geometry(&self, which: RegistrationGeometry) -> Option<&Geometry> {
        match which {
            RegistrationGeometry::Location => self.location.as_ref(),
            RegistrationGeometry::ObservationSpace => self.observation_space.as_ref(),
            RegistrationGeometry::OperationSpace => self.operation_space.as_ref(),
        }
    }

    /// Whether the source may hold an Entity `geo` matches: the geometry the
    /// geoquery names directly matches it or possibly contains a location
    /// that would (clauses 5.10.2.4 and 5.11.2.4).
    #[must_use]
    pub fn admits_geo_query(&self, geo: &GeoQuery) -> bool {
        geo.admits_registration_area(self.geometry(RegistrationGeometry::of(geo)))
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::coverage::RegistrationCoverage;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_query::geo::{query::GeoQuery, registration_area::RegistrationGeometry};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn point(longitude: f64, latitude: f64) -> Geometry {
        serde_json::from_value(json!({"type": "Point", "coordinates": [longitude, latitude]})).expect("a valid point")
    }

    fn near_madrid(geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel: "near;maxDistance==100000".parse().expect("a valid georel"),
            geometry: point(-3.7, 40.4),
            geoproperty: geoproperty.parse().expect("a valid attribute name"),
        }
    }

    /// A source located in Berlin whose observations cover Madrid.
    fn berlin_observing_madrid() -> RegistrationCoverage {
        RegistrationCoverage::builder()
            .information(Vec::new())
            .location(Some(point(13.4, 52.5)))
            .observation_space(Some(point(-3.7, 40.4)))
            .build()
    }

    #[test]
    fn each_registration_geometry_is_read_from_its_own_member() {
        let coverage = berlin_observing_madrid();
        assert_eq!(coverage.geometry(RegistrationGeometry::Location), Some(&point(13.4, 52.5)));
        assert_eq!(coverage.geometry(RegistrationGeometry::ObservationSpace), Some(&point(-3.7, 40.4)));
        assert_eq!(coverage.geometry(RegistrationGeometry::OperationSpace), None);
    }

    /// Clause 5.10.2.4: the geoquery is matched against the geometry its
    /// `geoproperty` names, `location` by default, and a geometry the
    /// registration does not declare may cover any area.
    #[test]
    fn a_geoquery_is_matched_against_the_geometry_its_geoproperty_names() {
        let coverage = berlin_observing_madrid();
        assert!(!coverage.admits_geo_query(&near_madrid("https://uri.etsi.org/ngsi-ld/location")));
        assert!(coverage.admits_geo_query(&near_madrid("https://uri.etsi.org/ngsi-ld/observationSpace")));
        assert!(coverage.admits_geo_query(&near_madrid("https://uri.etsi.org/ngsi-ld/operationSpace")));
    }
}
