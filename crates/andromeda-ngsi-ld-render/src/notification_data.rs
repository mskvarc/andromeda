//! The shape of a notification's `data` member (Table 5.3.1-1).
//!
//! "If the notification has been triggered from a Subscription that has the
//! notification.endpoint.accept field set to application/geo+json then data is
//! returned as a `FeatureCollection` [...] If endpoint.accept is not set or holds
//! another value then Entity[] is returned." Every sender of an entity
//! notification -- the change-driven one, the periodic one and the one that
//! re-assembles a forwarded copy -- asks this one function, so the three cannot
//! answer the same subscription in two shapes.

use crate::{error::RenderError, geojson::feature_collection, geometry_selection::GeometrySelection};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_ngsi_ld_model::notification::NotificationData;
use serde_json::Value;

/// The `data` of a notification serialised as `media_type`: the rendered
/// `entities` as they are, or one `GeoJSON` `FeatureCollection` of them for
/// `application/geo+json`, each geometry taken by `selection`.
///
/// A subscription names no `geometryProperty` (Table 5.2.14-1 has none), so a
/// notification takes the `location` `GeoProperty` clause 6.3.15 falls back to;
/// `selection` carries the subscription's `datasetId` for the instance.
///
/// # Errors
///
/// Returns [`RenderError::Serialization`] when the `FeatureCollection` cannot
/// be built.
pub fn notification_data(entities: Vec<Value>, media_type: ResponseMediaType, selection: GeometrySelection<'_>) -> Result<NotificationData, RenderError> {
    match media_type {
        ResponseMediaType::GeoJson => feature_collection(&entities, selection).map(NotificationData::FeatureCollection),
        ResponseMediaType::Json | ResponseMediaType::JsonLd => Ok(NotificationData::Entities(entities)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{geometry_selection::GeometrySelection, notification_data::notification_data};
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_model::notification::NotificationData;
    use serde_json::{Value, json};

    fn located_building() -> Value {
        json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [4.0, 52.0]}}
        })
    }

    /// Table 5.3.1-1: an endpoint that did not ask for `application/geo+json`
    /// receives the Entity array unchanged.
    #[test]
    fn a_json_or_json_ld_notification_carries_the_entity_array() {
        for media_type in [ResponseMediaType::Json, ResponseMediaType::JsonLd] {
            let data = notification_data(vec![located_building()], media_type, GeometrySelection::new(None, &[])).expect("data");
            pretty_assertions::assert_eq!(data, NotificationData::Entities(vec![located_building()]));
        }
    }

    /// Table 5.3.1-1: an `application/geo+json` endpoint receives one
    /// `FeatureCollection`, each Feature located by the `location` clause
    /// 6.3.15 falls back to.
    #[test]
    fn a_geo_json_notification_carries_one_feature_collection() {
        let data = notification_data(vec![located_building()], ResponseMediaType::GeoJson, GeometrySelection::new(None, &[])).expect("data");

        assert!(matches!(data, NotificationData::FeatureCollection(_)), "{data:?}");
        let rendered = serde_json::to_value(&data).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("FeatureCollection"));
        let features = rendered["features"].as_array().expect("features");
        pretty_assertions::assert_eq!(features.len(), 1);
        pretty_assertions::assert_eq!(features[0]["type"], json!("Feature"));
        pretty_assertions::assert_eq!(features[0]["geometry"], json!({"type": "Point", "coordinates": [4.0, 52.0]}));
    }
}
