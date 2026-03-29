use crate::{
    attribute::{
        Attribute,
        AttributeWrapper,
        geo_property::GeoProperty,
        json_property::JsonProperty,
        language_property::LanguageProperty,
        list_property::ListProperty,
        list_relationship::ListRelationship,
        property::Property,
        relationship::Relationship,
        vocab_property::VocabProperty,
    },
    context::NgsiLdContext,
    entity::{Entity, EntityType, Scope},
    name::NameBuf,
    representation::{NgsiLdRepresentation, NgsiLdSerializable, NgsiLdSkipNull},
    uri::Uri,
};
use chrono::{TimeZone, Utc};
use indexmap::IndexMap;
use ngsi_ld_value::Value;
use serde_json::{Value as JsonValue, json};
use urn::Urn;

fn create_test_entity(id: &str, entity_type: &str, key: &str, attr: Attribute) -> Entity {
    let mut attrs = IndexMap::new();
    attrs.insert(NameBuf::new(key).unwrap(), AttributeWrapper::Single(attr));

    Entity {
        context: Some(NgsiLdContext::String(
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_string(),
        )),
        id: Uri::Urn(format!("urn:ngsi-ld:{}:{}", entity_type, id).parse().unwrap()),
        type_: EntityType::Single(entity_type.to_string()),
        scope: None,
        attrs,
    }
}

fn serialize_entity(entity: &Entity, mode: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> JsonValue {
    entity.to_json(mode, skip_null).unwrap()
}

mod property_tests {
    use super::*;

    #[test]
    fn simple_property_all_modes() {
        let prop = Attribute::Property(Property::new(Value::from(json!("John Doe"))));
        let entity = create_test_entity("001", "Person", "name", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["name"]["type"], "Property");
        assert_eq!(normalized["name"]["value"], "John Doe");
        assert!(!normalized["name"].as_object().unwrap().contains_key("observedAt"));

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["name"], "John Doe");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["name"], "John Doe");
    }

    #[test]
    fn property_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let prop = Attribute::Property(Property {
            value: Value::from(json!(25)),
            observed_at: Some(dt),
            unit_code: Some("CEL".to_string()),
            dataset_id: Some("urn:dataset:temp".to_string()),
            instance_id: Some("urn:instance:123".to_string()),
            attributes: IndexMap::new(),
        });
        let entity = create_test_entity("001", "Sensor", "temperature", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["temperature"]["type"], "Property");
        assert_eq!(normalized["temperature"]["value"], 25);
        assert_eq!(normalized["temperature"]["unitCode"], "CEL");
        assert_eq!(normalized["temperature"]["datasetId"], "urn:dataset:temp");
        assert_eq!(normalized["temperature"]["instanceId"], "urn:instance:123");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["temperature"]["value"], 25);
        assert_eq!(concise["temperature"]["unitCode"], "CEL");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["temperature"], 25);
    }

    #[test]
    fn property_with_nested_attributes() {
        let accuracy = Attribute::Property(Property {
            value: Value::from(json!(95)),
            observed_at: None,
            unit_code: Some("%".to_string()),
            dataset_id: None,
            instance_id: None,
            attributes: IndexMap::new(),
        });

        let mut nested = IndexMap::new();
        nested.insert(NameBuf::new("accuracy").unwrap(), Box::new(AttributeWrapper::Single(accuracy)));

        let main_prop = Attribute::Property(Property {
            value: Value::from(json!(100)),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            instance_id: None,
            attributes: nested,
        });
        let entity = create_test_entity("001", "Sensor", "reading", main_prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["reading"]["type"], "Property");
        assert_eq!(normalized["reading"]["value"], 100);
        assert_eq!(normalized["reading"]["accuracy"]["type"], "Property");
        assert_eq!(normalized["reading"]["accuracy"]["value"], json!(95));
        assert_eq!(normalized["reading"]["accuracy"]["unitCode"], "%");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["reading"]["value"], 100);
        assert_eq!(concise["reading"]["accuracy"]["value"], json!(95));
        assert_eq!(concise["reading"]["accuracy"]["unitCode"], "%");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["reading"], 100);
    }

    #[test]
    fn property_null_values_skip() {
        let prop = Attribute::Property(Property::new(Value::from(JsonValue::Null)));
        let entity = create_test_entity("001", "Test", "nullValue", prop);

        let include_null = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Include);
        assert_eq!(include_null["nullValue"]["type"], "Property");
        assert!(include_null["nullValue"]["value"].is_null());

        let skip_null = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert!(!skip_null.as_object().unwrap().contains_key("nullValue"));
    }

    #[test]
    fn property_numeric_types() {
        let int_prop = Attribute::Property(Property::new(Value::from(json!(42))));
        let entity = create_test_entity("001", "Test", "intVal", int_prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["intVal"]["value"], 42);

        let float_prop = Attribute::Property(Property::new(Value::from(json!(3.14))));
        let entity2 = create_test_entity("002", "Test", "floatVal", float_prop);

        let normalized2 = serialize_entity(&entity2, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized2["floatVal"]["value"], 3.14);
    }

    #[test]
    fn property_boolean_value() {
        let prop = Attribute::Property(Property::new(Value::from(json!(true))));
        let entity = create_test_entity("001", "Test", "active", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["active"]["value"], true);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["active"], true);
    }

    #[test]
    fn property_object_value() {
        let obj_val = json!({"nested": "object", "count": 42});
        let prop = Attribute::Property(Property::new(Value::from(obj_val.clone())));
        let entity = create_test_entity("001", "Test", "complex", prop);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["complex"]["value"]["nested"], "object");
        assert_eq!(normalized["complex"]["value"]["count"], 42);
    }
}

mod relationship_tests {
    use super::*;

    #[test]
    fn simple_relationship_all_modes() {
        let rel = Attribute::Relationship(Relationship::new("urn:ngsi-ld:Vehicle:car123".parse::<Urn>().unwrap()));
        let entity = create_test_entity("001", "Person", "drives", rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["drives"]["type"], "Relationship");
        assert_eq!(normalized["drives"]["object"], "urn:ngsi-ld:Vehicle:car123");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["drives"], "urn:ngsi-ld:Vehicle:car123");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["drives"], "urn:ngsi-ld:Vehicle:car123");
    }

    #[test]
    fn relationship_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let mut nested = IndexMap::new();
        nested.insert(
            NameBuf::new("count").unwrap(),
            Box::new(AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(json!(5)))))),
        );

        let rel = Attribute::Relationship(Relationship {
            object: "urn:ngsi-ld:Organization:Beatles".parse::<Urn>().unwrap(),
            observed_at: Some(dt),
            dataset_id: Some("urn:ngsi-ld:Relationship:Beatles".to_string()),
            instance_id: None,
            attributes: nested,
        });
        let entity = create_test_entity("001", "Person", "memberOf", rel);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["memberOf"]["type"], "Relationship");
        assert_eq!(normalized["memberOf"]["object"], "urn:ngsi-ld:Organization:Beatles");
        assert_eq!(normalized["memberOf"]["count"]["type"], "Property");
        assert_eq!(normalized["memberOf"]["count"]["value"], 5);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["memberOf"]["object"], "urn:ngsi-ld:Organization:Beatles");
        assert_eq!(concise["memberOf"]["count"], 5);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["memberOf"], "urn:ngsi-ld:Organization:Beatles");
    }
}

mod geo_property_tests {
    use super::*;

    #[test]
    fn geo_property_all_modes() {
        let geo_value = Value::from(json!({
            "type": "Point",
            "coordinates": [-73.975, 40.775556]
        }));
        let geo = Attribute::GeoProperty(GeoProperty::new(geo_value));
        let entity = create_test_entity("001", "Place", "location", geo);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["location"]["type"], "GeoProperty");
        assert_eq!(normalized["location"]["value"]["type"], "Point");
        assert_eq!(normalized["location"]["value"]["coordinates"][0], -73.975);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["location"]["type"], "Point");
        assert_eq!(concise["location"]["coordinates"][0], -73.975);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["location"]["type"], "Point");
    }

    #[test]
    fn geo_property_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let geo = Attribute::GeoProperty(GeoProperty {
            value: Value::from(json!({"type": "Point", "coordinates": [0.0, 0.0]})),
            observed_at: Some(dt),
            dataset_id: Some("urn:dataset:geo".to_string()),
            instance_id: None,
        });
        let entity = create_test_entity("001", "Place", "area", geo);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["area"].as_object().unwrap().contains_key("value"));
        assert!(concise["area"].as_object().unwrap().contains_key("observedAt"));

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["area"]["type"], "Point");
    }
}

mod language_property_tests {
    use super::*;

    #[test]
    fn language_property_all_modes() {
        let mut lang_map = IndexMap::new();
        lang_map.insert("en".to_string(), "Main Street".to_string());
        lang_map.insert("de".to_string(), "Hauptstrasse".to_string());

        let lp = Attribute::LanguageProperty(LanguageProperty::new(lang_map));
        let entity = create_test_entity("001", "Building", "street", lp);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["street"]["type"], "LanguageProperty");
        assert_eq!(normalized["street"]["languageMap"]["en"], "Main Street");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["street"]["en"], "Main Street");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["street"]["en"], "Main Street");
    }

    #[test]
    fn language_property_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut lang_map = IndexMap::new();
        lang_map.insert("en".to_string(), "Hello".to_string());

        let lp = Attribute::LanguageProperty(LanguageProperty {
            language_map: lang_map,
            observed_at: Some(dt),
            dataset_id: None,
            attributes: IndexMap::new(),
        });
        let entity = create_test_entity("001", "Greeting", "message", lp);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert!(concise["message"].as_object().unwrap().contains_key("languageMap"));
        assert!(concise["message"].as_object().unwrap().contains_key("observedAt"));
    }
}

mod vocab_property_tests {
    use super::*;

    #[test]
    fn vocab_property_all_modes() {
        let vocab = Attribute::VocabProperty(VocabProperty::new("electricMicrobus"));
        let entity = create_test_entity("001", "Vehicle", "vehicleType", vocab);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["vehicleType"]["type"], "VocabProperty");
        assert_eq!(normalized["vehicleType"]["hasVocab"], "electricMicrobus");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["vehicleType"], "electricMicrobus");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["vehicleType"], "electricMicrobus");
    }

    #[test]
    fn vocab_property_with_metadata() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 0).unwrap();
        let vocab = Attribute::VocabProperty(VocabProperty {
            has_vocab: "electricMicrobus".to_string(),
            observed_at: Some(dt),
            dataset_id: Some("urn:dataset:vocab".to_string()),
            attributes: IndexMap::new(),
        });
        let entity = create_test_entity("001", "Vehicle", "vehicleType", vocab);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["vehicleType"]["hasVocab"], "electricMicrobus");
        assert!(concise["vehicleType"].as_object().unwrap().contains_key("observedAt"));
    }
}

mod list_property_tests {
    use super::*;

    #[test]
    fn list_property_all_modes() {
        let lp = Attribute::ListProperty(ListProperty::new(vec![Value::from(json!(124)), Value::from(json!(156))]));
        let entity = create_test_entity("001", "TrafficFlowObserved", "hourlyVehicleCounts", lp);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["hourlyVehicleCounts"]["type"], "ListProperty");
        assert_eq!(normalized["hourlyVehicleCounts"]["hasValueList"], json!([124, 156]));

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["hourlyVehicleCounts"], json!([124, 156]));

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["hourlyVehicleCounts"], json!([124, 156]));
    }
}

mod list_relationship_tests {
    use super::*;

    #[test]
    fn list_relationship_all_modes() {
        let lr = Attribute::ListRelationship(ListRelationship::new(vec![
            "urn:ngsi-ld:Person:John".parse::<Urn>().unwrap(),
            "urn:ngsi-ld:Person:Paul".parse::<Urn>().unwrap(),
        ]));
        let entity = create_test_entity("001", "Group", "members", lr);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["members"]["type"], "ListRelationship");
        assert_eq!(normalized["members"]["objectList"][0], "urn:ngsi-ld:Person:John");
        assert_eq!(normalized["members"]["objectList"][1], "urn:ngsi-ld:Person:Paul");

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["members"]["objectList"][0], "urn:ngsi-ld:Person:John");

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["members"][0], "urn:ngsi-ld:Person:John");
    }
}

mod json_property_tests {
    use super::*;

    #[test]
    fn json_property_all_modes() {
        let json_val = json!({"uptime_hrs": 4102, "battery_mv": 3200});
        let jp = Attribute::JsonProperty(JsonProperty::new(Value::from(json_val.clone())));
        let entity = create_test_entity("001", "ParkingSpot", "sensorDiagnostics", jp);

        let normalized = serialize_entity(&entity, NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip);
        assert_eq!(normalized["sensorDiagnostics"]["type"], "JsonProperty");
        assert_eq!(normalized["sensorDiagnostics"]["hasJson"], json_val);

        let concise = serialize_entity(&entity, NgsiLdRepresentation::Concise, NgsiLdSkipNull::Skip);
        assert_eq!(concise["sensorDiagnostics"], json_val);

        let simplified = serialize_entity(&entity, NgsiLdRepresentation::Simplified, NgsiLdSkipNull::Skip);
        assert_eq!(simplified["sensorDiagnostics"], json_val);
    }
}

mod multi_instance_tests {
    use super::*;

    #[test]
    fn multi_instance_attribute() {
        let attrs = AttributeWrapper::Multi(vec![
            Attribute::Property(Property::new(Value::from(json!("Eiffel Tower")))),
            Attribute::Property(Property {
                value: Value::from(json!("Tour Eiffel")),
                observed_at: None,
                unit_code: None,
                dataset_id: Some("urn:ngsi-ld:Dataset:french".to_string()),
                instance_id: None,
                attributes: IndexMap::new(),
            }),
        ]);

        let mut entity = Entity::new(
            Uri::Urn("urn:ngsi-ld:Building:002".parse().unwrap()),
            EntityType::Single("Building".to_string()),
        );
        entity.attrs.insert(NameBuf::new("name").unwrap(), attrs);

        let normalized = entity.to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip).unwrap();
        assert!(normalized["name"].is_array());
        assert_eq!(normalized["name"][0]["type"], "Property");
        assert_eq!(normalized["name"][0]["value"], "Eiffel Tower");
        assert_eq!(normalized["name"][1]["value"], "Tour Eiffel");
        assert_eq!(normalized["name"][1]["datasetId"], "urn:ngsi-ld:Dataset:french");
    }
}

mod entity_scope_tests {
    use super::*;

    #[test]
    fn entity_with_scope_string() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "scope": "/Madrid/Gardens/ParqueNorte"
        });
        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert!(entity.scope.is_some());
    }

    #[test]
    fn entity_with_scope_array() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "scope": ["/Madrid", "/Barcelona"]
        });
        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert!(matches!(entity.scope, Some(Scope::Multiple(_))));
    }
}

mod deserialization_tests {
    use super::*;

    #[test]
    fn normalized_entity_round_trips() {
        let mut entity = Entity::new(Uri::Urn("urn:ngsi-ld:Sensor:001".parse().unwrap()), EntityType::Single("Sensor".to_string()));
        entity.context = Some(NgsiLdContext::String("https://schema.org".to_string()));
        entity.attrs.insert(
            NameBuf::new("temperature").unwrap(),
            AttributeWrapper::Single(Attribute::Property(Property::new(Value::from(json!(25.5))))),
        );

        let json = entity.to_json(NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip).unwrap();
        let json_str = serde_json::to_string(&json).unwrap();
        let parsed: Entity = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.id, entity.id);
        assert_eq!(parsed.type_, entity.type_);
        assert!(parsed.attrs.contains_key(&NameBuf::new("temperature").unwrap()));
    }

    #[test]
    fn deserializes_all_attribute_types() {
        let payload = json!({
            "id": "urn:ngsi-ld:Test:all",
            "type": "TestEntity",
            "prop": {"type": "Property", "value": "hello"},
            "rel": {"type": "Relationship", "object": "urn:ngsi-ld:Thing:1"},
            "geo": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
            "lang": {"type": "LanguageProperty", "languageMap": {"en": "Hello", "es": "Hola"}},
            "vocab": {"type": "VocabProperty", "hasVocab": "term1"},
            "jsonp": {"type": "JsonProperty", "hasJson": {"key": "val"}}
        });

        let entity: Entity = serde_json::from_value(payload).unwrap();
        assert_eq!(entity.attrs.len(), 6);
    }
}
