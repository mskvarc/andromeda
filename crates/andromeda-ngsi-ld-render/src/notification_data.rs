//! A notification's `data` member (Table 5.3.1-1), written once.
//!
//! "If the notification has been triggered from a Subscription that has the
//! notification.endpoint.accept field set to application/geo+json then data is
//! returned as a `FeatureCollection` [...] If endpoint.accept is not set or holds
//! another value then Entity[] is returned." Every sender of an entity
//! notification -- the change-driven one, the periodic one and the one that
//! re-assembles a forwarded copy -- writes its entities through this one
//! function, so the three cannot answer the same subscription in two shapes.
//!
//! The `@context` is placed as it is written. Clause 6.3.8 puts it in the body
//! of an `application/ld+json` notification and, unless the endpoint's
//! `receiverInfo` prefers `body=json`, of an `application/geo+json` one; where
//! it goes inside `data` follows from the shape. Every Entity of an array is a
//! document of its own (clause 4.5.1 gives it an `@id` and an `@type`), so each
//! carries the context, while a `FeatureCollection` carries it once and no
//! Feature does (clauses 4.5.16.3 and 4.5.17.2: "separate @context elements for
//! each Feature will not be present").

use crate::{
    entity_body::{entity_collection::EntityCollection, feature_body::FeatureBody, feature_collection::FeatureCollection, typed_entity::TypedEntity},
    error::{RenderError, RenderedShape},
    geometry_selection::GeometrySelection,
    notification_entity::NotificationEntity,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_model::notification::NotificationData;
use serde_json::value::to_raw_value;

/// How a notification's `data` is written for its endpoint.
#[derive(Debug, Clone, Copy)]
pub struct DataShape<'a> {
    /// The media type the notification is sent as.
    pub media_type: ResponseMediaType,
    /// The clause 4.5.16.1 selection a Feature's geometry is taken by.
    ///
    /// A subscription names no `geometryProperty` (Table 5.2.14-1 has none),
    /// so a notification takes the `location` `GeoProperty` clause 6.3.15 falls
    /// back to; the selection carries the subscription's `datasetId` for the
    /// instance.
    pub selection: GeometrySelection<'a>,
    /// The `@context` the body carries, when clause 6.3.8 places it there.
    pub context: Option<&'a JsonLiteral>,
}

/// The `data` of a notification carrying `entities`, written as `shape`
/// says: the Entity array, or one `GeoJSON` `FeatureCollection` of them for
/// `application/geo+json`.
///
/// # Errors
///
/// Returns [`RenderError::Serialization`] when an entity, a Feature or the
/// `FeatureCollection` cannot be written.
pub fn notification_data(entities: &[NotificationEntity<'_>], shape: DataShape<'_>) -> Result<NotificationData, RenderError> {
    let written = match shape.media_type {
        ResponseMediaType::Json | ResponseMediaType::JsonLd => {
            let entities: Vec<TypedEntity<'_>> = entities.iter().map(|entity| entity.typed(None)).collect();
            to_raw_value(&EntityCollection {
                entities: &entities,
                context: shape.context,
            })
            .map_err(|error| RenderError::serialization(RenderedShape::Entity, error))
        }
        ResponseMediaType::GeoJson => entities
            .iter()
            .map(|entity| FeatureBody::new(&entity.typed(None), shape.selection, None))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|features| {
                to_raw_value(&FeatureCollection {
                    features,
                    context: shape.context,
                })
            })
            .map_err(|error| RenderError::serialization(RenderedShape::GeoJsonFeatureCollection, error)),
    };
    written.map(NotificationData::written)
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry_selection::GeometrySelection,
        notification_data::{DataShape, notification_data},
        notification_entity::{NotificationEntity, StampedInstants, TargetSpelling},
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    fn located_building(id: &str) -> Entity {
        serde_json::from_value(json!({
            "id": id,
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [4.0, 52.0]}}
        }))
        .expect("a Building")
    }

    fn context() -> JsonLiteral {
        JsonLiteral::admit(r#"["https://example.com/ctx.jsonld","https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"]"#).expect("a context")
    }

    /// The `data` two Buildings are written into for `media_type`, carrying
    /// `context` when the body does.
    fn written(media_type: ResponseMediaType, context: Option<&JsonLiteral>) -> Value {
        let buildings = [located_building("urn:ngsi-ld:Building:1"), located_building("urn:ngsi-ld:Building:2")];
        let stamped = StampedInstants::default();
        let entities: Vec<NotificationEntity<'_>> = buildings
            .iter()
            .map(|building| NotificationEntity::new(building, &stamped, ResponseFormat::Normalized, TargetSpelling::Decoded))
            .collect();
        let shape = DataShape {
            media_type,
            selection: GeometrySelection::new(None, &[]),
            context,
        };
        let data = notification_data(&entities, shape).expect("the data is written");
        serde_json::from_str(data.text()).expect("the written data is JSON")
    }

    /// Table 5.3.1-1: an endpoint that did not ask for `application/geo+json`
    /// receives the Entity array, and a body that carries no context names it
    /// on no entity.
    #[test]
    fn a_json_notification_carries_the_entity_array_without_a_context() {
        let data = written(ResponseMediaType::Json, None);
        let entities = data.as_array().expect("an Entity array");
        pretty_assertions::assert_eq!(entities.len(), 2);
        for entity in entities {
            pretty_assertions::assert_eq!(entity["type"], json!("Building"));
            assert!(entity.get("@context").is_none(), "{entity}");
        }
    }

    /// Clause 6.3.6 with clause 4.5.1: every Entity of an `application/ld+json`
    /// notification is a document of its own and carries the body's context.
    #[test]
    fn a_json_ld_notification_writes_the_context_on_every_entity() {
        let context = context();
        let data = written(ResponseMediaType::JsonLd, Some(&context));
        let expected: Value = serde_json::from_str(context.text()).expect("the context is JSON");
        for entity in data.as_array().expect("an Entity array") {
            pretty_assertions::assert_eq!(entity["@context"], expected, "{entity}");
        }
    }

    /// Table 5.3.1-1 and clause 4.5.16.3: an `application/geo+json` endpoint
    /// receives one `FeatureCollection` carrying the context, located by the
    /// `location` clause 6.3.15 falls back to, and no Feature carries one.
    #[test]
    fn a_geo_json_notification_carries_one_feature_collection_naming_the_context_once() {
        let context = context();
        let data = written(ResponseMediaType::GeoJson, Some(&context));
        pretty_assertions::assert_eq!(data["type"], json!("FeatureCollection"));
        pretty_assertions::assert_eq!(data["@context"], serde_json::from_str::<Value>(context.text()).expect("the context is JSON"));
        let features = data["features"].as_array().expect("features");
        pretty_assertions::assert_eq!(features.len(), 2);
        for feature in features {
            pretty_assertions::assert_eq!(feature["type"], json!("Feature"));
            pretty_assertions::assert_eq!(feature["geometry"], json!({"type": "Point", "coordinates": [4.0, 52.0]}));
            assert!(feature.get("@context").is_none(), "{feature}");
            assert!(feature["properties"].get("@context").is_none(), "{feature}");
        }
    }

    /// Table 5.3.1-1: with `Prefer: body=json` "the `FeatureCollection` will
    /// not contain an @context field".
    #[test]
    fn a_geo_json_notification_without_a_body_context_names_none() {
        let data = written(ResponseMediaType::GeoJson, None);
        assert!(data.get("@context").is_none(), "{data}");
    }
}
