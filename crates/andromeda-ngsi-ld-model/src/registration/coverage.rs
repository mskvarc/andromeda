//! What a registered Context Source answers for, in space, time and selection (ETSI GS CIM 009 clauses 5.2.9 and 5.2.10).

use crate::{registration::info::RegistrationInfo, time_interval::TimeInterval};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{scope::path::ScopePath, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    csr::compiled::CompiledCsrTemporalFilter,
    dataset::DatasetSelector,
    geo::{query::GeoQuery, registration_area::RegistrationGeometry},
    scope::query::ScopeQuery,
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

    /// Whether the source may hold an Entity `scope_q` matches: the query
    /// matches the registration's `scope` (clauses 4.19, 5.10.2.4 and
    /// 5.11.2.4). A registration declaring no scope has nothing the query
    /// could match, as an Entity without a scope matches none (clause 4.19).
    #[must_use]
    pub fn admits_scope_query(&self, scope_q: &ScopeQuery) -> bool {
        self.scope.as_deref().is_some_and(|scopes| scope_q.matches(scopes))
    }

    /// Whether the registration falls on the side of the temporal partition
    /// `temporal_q` selects (clauses 5.10.2.4 and 5.11.2.4): without a
    /// temporal query only a registration declaring no interval, which
    /// provides latest information; with one, the interval its
    /// `timeproperty` selects must be declared and meet the query window.
    #[must_use]
    pub fn admits_temporal(&self, temporal_q: Option<&CompiledCsrTemporalFilter>) -> bool {
        let Some(temporal_q) = temporal_q else {
            return self.observation_interval.is_none() && self.management_interval.is_none();
        };
        let relevant = match temporal_q.timeproperty.unwrap_or(TimeProp::Observed) {
            TimeProp::Observed => self.observation_interval.as_ref(),
            TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => self.management_interval.as_ref(),
        };
        relevant.is_some_and(|interval| interval.meets(&temporal_q.interval))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{registration::coverage::RegistrationCoverage, time_interval::TimeInterval};
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{scope::path::ScopePath, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        csr::compiled::CompiledCsrTemporalFilter,
        geo::{query::GeoQuery, registration_area::RegistrationGeometry},
        scope::query::ScopeQuery,
        temporal::window::TimeInterval as QueryWindow,
    };
    use chrono::{DateTime, Utc};
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

    fn scoped(scopes: Option<&[&str]>) -> RegistrationCoverage {
        RegistrationCoverage::builder()
            .information(Vec::new())
            .scope(scopes.map(|scopes| scopes.iter().map(|scope| scope.parse::<ScopePath>().expect("a valid scope")).collect()))
            .build()
    }

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn interval(start_at: &str, end_at: &str) -> TimeInterval {
        TimeInterval {
            start_at: at(start_at),
            end_at: Some(at(end_at)),
        }
    }

    fn temporal_q(window: QueryWindow, timeproperty: Option<TimeProp>) -> CompiledCsrTemporalFilter {
        CompiledCsrTemporalFilter::builder().interval(window).timeproperty(timeproperty).build()
    }

    /// Clause 4.19: a registration declaring no scope has nothing a Scope
    /// query could match, whatever the query.
    #[test]
    fn an_unscoped_registration_admits_no_scope_query() {
        let unscoped = scoped(None);
        for query in ["/Madrid", "/Madrid/#", "/#"] {
            assert!(!unscoped.admits_scope_query(&ScopeQuery::parse(query).expect("a valid scope query")), "{query}");
        }
    }

    /// Clauses 5.10.2.4 and 5.11.2.4: the Scope query is matched against the
    /// registration's `scope`.
    #[test]
    fn a_scoped_registration_admits_a_matching_scope_query() {
        let gardens = scoped(Some(&["/Madrid/Gardens"]));
        assert!(gardens.admits_scope_query(&ScopeQuery::parse("/Madrid/#").expect("a valid scope query")));
        assert!(gardens.admits_scope_query(&ScopeQuery::parse("/Madrid/Gardens").expect("a valid scope query")));
        assert!(!gardens.admits_scope_query(&ScopeQuery::parse("/Berlin/#").expect("a valid scope query")));
    }

    /// Clauses 5.10.2.4 and 5.11.2.4: without a temporal query only a
    /// latest-information registration is selected; with one, only a
    /// registration declaring the interval the `timeproperty` selects, and
    /// only when that interval meets the window.
    #[test]
    fn the_temporal_partition_table() {
        let latest = RegistrationCoverage::builder().information(Vec::new()).build();
        let observed = RegistrationCoverage::builder()
            .information(Vec::new())
            .observation_interval(Some(interval("2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z")))
            .build();
        let managed = RegistrationCoverage::builder()
            .information(Vec::new())
            .management_interval(Some(interval("2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z")))
            .build();

        assert!(latest.admits_temporal(None));
        assert!(!observed.admits_temporal(None));
        assert!(!managed.admits_temporal(None));

        let after_observed = temporal_q(QueryWindow::After(at("2020-06-01T00:00:00Z")), None);
        assert!(!latest.admits_temporal(Some(&after_observed)));
        assert!(observed.admits_temporal(Some(&after_observed)));
        assert!(!managed.admits_temporal(Some(&after_observed)), "the relevant interval is absent");

        let after_created = temporal_q(QueryWindow::After(at("2020-06-01T00:00:00Z")), Some(TimeProp::Created));
        assert!(managed.admits_temporal(Some(&after_created)));
        assert!(!observed.admits_temporal(Some(&after_created)));

        let outside = temporal_q(QueryWindow::After(at("2022-06-01T00:00:00Z")), None);
        assert!(!observed.admits_temporal(Some(&outside)));

        let overlapping = temporal_q(
            QueryWindow::Between {
                from: at("2020-06-01T00:00:00Z"),
                to: at("2020-09-01T00:00:00Z"),
            },
            None,
        );
        assert!(observed.admits_temporal(Some(&overlapping)));
    }
}
