//! The geometries an entity's `GeoProperty` attributes carry (clauses 4.7,
//! 4.10, 4.5.16.1).
//!
//! A geo-query is met when any instance of the target `GeoProperty` meets it
//! (clause 4.10), so matching reads every instance. The spatial index and the
//! `GeoJSON` representation read one instance, the default one of clause
//! 4.5.16.1: the instance without a `datasetId`, else the first.

use crate::{attribute_instances::AttributeInstances, entity_state::EntityState, geo_value::GeoValue};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};

impl EntityState {
    /// The geometry of every instance of the `GeoProperty` `name`, in stored
    /// order (clause 4.10 any-instance matching).
    pub fn geo_property_geometries(&self, name: &str) -> impl Iterator<Item = &GeometryForm> {
        self.attributes()
            .get(name)
            .into_iter()
            .flat_map(AttributeInstances::iter)
            .filter_map(|(_, instance)| instance.value().geometry().map(GeoValue::form))
    }

    /// The geometry of the default instance of the `GeoProperty` `name`
    /// (clause 4.5.16.1).
    #[must_use]
    pub fn geo_property_geometry(&self, name: &str) -> Option<&GeometryForm> {
        let (_, instance) = self.attributes().get(name)?.default_instance()?;
        instance.value().geometry().map(GeoValue::form)
    }

    /// The default `location` geometry.
    #[must_use]
    pub fn location_geometry(&self) -> Option<&GeometryForm> {
        self.geo_property_geometry(properties::LOCATION.as_str())
    }

    /// The expiry of the default `location` instance (clause 4.22): the same
    /// instance the hoisted geometry is read from.
    #[must_use]
    pub fn location_expiry(&self) -> Option<DateTime<Utc>> {
        let (_, instance) = self.attributes().get(properties::LOCATION.as_str())?.default_instance()?;
        instance.expires_at().map(NgsiLdDateTime::get)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_state::EntityState;
    use andromeda_ngsi_ld_geometry::form::GeometryForm;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn point(longitude: f64, latitude: f64) -> Value {
        json!({"@type": ["https://purl.org/geojson/vocab#Point"], "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]})
    }

    fn instance(value: &Value, dataset: Option<&str>, expires_at: Option<&str>) -> Value {
        let mut instance = json!({"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"], "https://uri.etsi.org/ngsi-ld/hasValue": [value]});
        if let Some(dataset) = dataset {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        if let Some(expires_at) = expires_at {
            instance["https://uri.etsi.org/ngsi-ld/expiresAt"] = json!([{"@value": expires_at}]);
        }
        instance
    }

    fn state(attribute: &str, instances: &Value) -> EntityState {
        let document = json!([{"@id": "urn:ngsi-ld:Building:001", "@type": ["https://ex.org/Building"], attribute: instances}]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a state")
    }

    fn longitude(form: Option<&GeometryForm>) -> Option<f64> {
        match form? {
            GeometryForm::Point(position) => Some(position.longitude()),
            GeometryForm::MultiPoint(_)
            | GeometryForm::LineString(_)
            | GeometryForm::MultiLineString(_)
            | GeometryForm::Polygon(_)
            | GeometryForm::MultiPolygon(_) => None,
        }
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("instant parses").get()
    }

    #[test]
    fn an_entity_without_a_location_has_no_location_geometry_or_expiry() {
        let entity = state("https://ex.org/other", &json!([instance(&point(1.0, 2.0), None, None)]));
        assert!(entity.location_geometry().is_none());
        pretty_assertions::assert_eq!(entity.location_expiry(), None);
        pretty_assertions::assert_eq!(entity.geo_property_geometries(LOCATION).count(), 0);
    }

    /// Clause 4.22 invalidates one instance at its own `expiresAt`, so the
    /// expiry beside the hoisted default instance is that instance's own.
    #[test]
    fn the_default_instances_own_expiry_is_recorded_whatever_the_other_instances_carry() {
        let entity = state(
            LOCATION,
            &json!([
                instance(&point(0.0, 0.0), Some("urn:ngsi-ld:Dataset:a"), Some("2031-01-01T00:00:00Z")),
                instance(&point(1.0, 1.0), None, Some("2030-01-01T00:00:00Z"))
            ]),
        );
        pretty_assertions::assert_eq!(entity.location_expiry(), Some(instant("2030-01-01T00:00:00Z")));
    }

    #[test]
    fn with_every_instance_tagged_the_first_instances_expiry_and_geometry_are_read() {
        let entity = state(
            LOCATION,
            &json!([
                instance(&point(13.4, 52.5), Some("urn:ngsi-ld:Dataset:A"), Some("2030-01-01T00:00:00Z")),
                instance(&point(0.0, 0.0), Some("urn:ngsi-ld:Dataset:B"), None)
            ]),
        );
        pretty_assertions::assert_eq!(entity.location_expiry(), Some(instant("2030-01-01T00:00:00Z")));
        pretty_assertions::assert_eq!(longitude(entity.location_geometry()), Some(13.4));
    }

    /// Clause 4.10: any instance meets the restriction, so every instance is
    /// yielded for matching, while the index reads the default one.
    #[test]
    fn every_instance_of_a_multi_dataset_geoproperty_is_yielded_for_matching() {
        let entity = state(
            LOCATION,
            &json!([
                instance(&point(0.0, 0.0), Some("urn:ngsi-ld:Dataset:tagged"), None),
                instance(&point(13.4, 52.5), None, None)
            ]),
        );
        let longitudes: Vec<Option<f64>> = entity.geo_property_geometries(LOCATION).map(|form| longitude(Some(form))).collect();
        pretty_assertions::assert_eq!(longitudes, vec![Some(0.0), Some(13.4)]);
        pretty_assertions::assert_eq!(longitude(entity.location_geometry()), Some(13.4));
    }

    #[test]
    fn a_geoproperty_written_as_a_json_string_is_read_as_a_geometry() {
        let entity = state(
            LOCATION,
            &json!([instance(&json!({"@value": "{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}"}), None, None)]),
        );
        pretty_assertions::assert_eq!(longitude(entity.location_geometry()), Some(13.4));
    }

    #[test]
    fn the_expanded_node_form_is_read_for_an_arbitrary_geoproperty_iri() {
        let entity = state("https://example.com/site", &json!([instance(&point(16.37, 48.2), None, None)]));
        pretty_assertions::assert_eq!(longitude(entity.geo_property_geometry("https://example.com/site")), Some(16.37));
    }
}
