//! The clause 4.3.6.8 fallback tables applied to a forwarded write payload bound
//! for an older source: entity- and attribute-level removals (Table 4.3.6.8-1),
//! property sub-attribute removals (Table 4.3.6.8-2), and relationship
//! sub-attribute removals (Table 4.3.6.8-3). All `T-MOCK`; `UNKNOWN` status,
//! each classifying on first run.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity create.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// The `contextSourceInfo` key by which a registration declares the NGSI-LD
/// conformance version of its source.
const CONFORMANCE_KEY: &str = "ngsild-conformance";

/// Provision an entity through an inclusive registration declaring an older
/// source conformance version and return the exact payload forwarded to the
/// source, so a downgrade transform can be asserted.
async fn forwarded_downgrade_body(name: &str, conformance: &str, make_entity: impl FnOnce(&str) -> Value) -> Value {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let entity_id = topology.id(&format!("Vehicle:{name}"));
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id(&format!("registration:{name}")), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["createEntity"])
        .context_source_info(CONFORMANCE_KEY, conformance)
        .build();
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("create registration"),
        201,
    );

    let body = make_entity(&entity_id);
    let created = topology.front().client().post_entity(&body).with_ld_json().send().await.expect("provision");
    assert!(created.status.is_success(), "the provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_body: Value = forwarded.json().expect("forwarded body is JSON");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
    forwarded_body
}

/// An attribute-level `expiresAt` is removed for an older source (Table
/// 4.3.6.8-1). Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_expires_at_removed() {
    let forwarded = forwarded_downgrade_body("bc-attr-expiresat", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[("temperature", json!({ "type": "Property", "value": 5, "expiresAt": "2099-01-01T00:00:00Z" }))],
        )
    })
    .await;
    assert!(
        forwarded["temperature"].get("expiresAt").is_none(),
        "the attribute-level expiresAt is removed: {}",
        forwarded["temperature"]
    );
}

/// An entity-level `scope` is removed for an older source (Table 4.3.6.8-1).
/// Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_scope_removed() {
    let forwarded = forwarded_downgrade_body("bc-attr-scope", "1.0", |id| {
        let mut body = entity(id, "Vehicle", &[("speed", property(json!(10)))]);
        body["scope"] = json!("/Madrid");
        body
    })
    .await;
    assert!(forwarded.get("scope").is_none(), "the entity-level scope is removed: {forwarded}");
}

/// A `LanguageProperty` is downgraded to a `Property` for an older source (Table
/// 4.3.6.8-1). Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_language_property_downgraded_to_property() {
    let forwarded = forwarded_downgrade_body("bc-attr-language", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[("label", json!({ "type": "LanguageProperty", "languageMap": { "en": "car", "es": "coche" } }))],
        )
    })
    .await;
    assert_eq!(
        forwarded["label"]["type"].as_str(),
        Some("Property"),
        "a LanguageProperty downgrades to Property: {}",
        forwarded["label"]
    );
}

/// A `JsonProperty` is downgraded for an older source (Table 4.3.6.8-1).
/// Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_json_property() {
    let forwarded = forwarded_downgrade_body("bc-attr-json", "1.0", |id| {
        entity(id, "Vehicle", &[("payload", json!({ "type": "JsonProperty", "json": { "k": 1 } }))])
    })
    .await;
    assert_ne!(
        forwarded["payload"]["type"].as_str(),
        Some("JsonProperty"),
        "a JsonProperty is downgraded: {}",
        forwarded["payload"]
    );
}

/// A `VocabProperty` is downgraded for an older source (Table 4.3.6.8-1).
/// Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_vocab_property() {
    let forwarded = forwarded_downgrade_body("bc-attr-vocab", "1.0", |id| {
        entity(id, "Vehicle", &[("category", json!({ "type": "VocabProperty", "vocab": "sedan" }))])
    })
    .await;
    assert_ne!(
        forwarded["category"]["type"].as_str(),
        Some("VocabProperty"),
        "a VocabProperty is downgraded: {}",
        forwarded["category"]
    );
}

/// A `ListProperty` is downgraded for an older source (Table 4.3.6.8-1).
/// Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_list_property() {
    let forwarded = forwarded_downgrade_body("bc-attr-listprop", "1.0", |id| {
        entity(id, "Vehicle", &[("readings", json!({ "type": "ListProperty", "valueList": [1, 2, 3] }))])
    })
    .await;
    assert_ne!(
        forwarded["readings"]["type"].as_str(),
        Some("ListProperty"),
        "a ListProperty is downgraded: {}",
        forwarded["readings"]
    );
}

/// A `ListRelationship` is downgraded to a `Relationship` for an older source
/// (Table 4.3.6.8-1). Confirms REQ-FW-59.
#[tokio::test]
async fn distop_entity_bc_attr_fallback_list_relationship_downgraded_to_relationship() {
    let forwarded = forwarded_downgrade_body("bc-attr-listrel", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "partners",
                json!({ "type": "ListRelationship", "objectList": ["urn:ngsi-ld:X:1", "urn:ngsi-ld:X:2"] }),
            )],
        )
    })
    .await;
    assert_eq!(
        forwarded["partners"]["type"].as_str(),
        Some("Relationship"),
        "a ListRelationship downgrades to Relationship: {}",
        forwarded["partners"]
    );
}

/// A multi-valued `type` is reduced to its first value for a v1.0 source (note
/// 1). Confirms REQ-FW-60.
#[tokio::test]
async fn distop_entity_bc_v1_0_multivalued_type_reduced_to_first() {
    let forwarded = forwarded_downgrade_body("bc-multitype", "1.0", |id| {
        let mut body = entity(id, "Vehicle", &[("speed", property(json!(10)))]);
        body["type"] = json!(["Vehicle", "Car"]);
        body
    })
    .await;
    assert_eq!(
        forwarded["type"].as_str(),
        Some("Vehicle"),
        "a multi-valued type reduces to its first value: {}",
        forwarded["type"]
    );
}

/// A multi-instance attribute is reduced to its default instance for a v1.0
/// source (notes 2-3). Confirms REQ-FW-61.
#[tokio::test]
async fn distop_entity_bc_v1_0_multi_instance_attr_reduced_to_default() {
    let forwarded = forwarded_downgrade_body("bc-multiinstance", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "speed",
                json!([
                    { "type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:ds:1" },
                    { "type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:2" }
                ]),
            )],
        )
    })
    .await;
    assert!(
        forwarded["speed"].is_object(),
        "a multi-instance attribute reduces to a single default instance: {}",
        forwarded["speed"]
    );
}

/// A property's `datasetId` sub-attribute is removed for an older source (Table
/// 4.3.6.8-2). Confirms REQ-FW-62.
#[tokio::test]
async fn distop_entity_bc_property_sub_attr_dataset_id_removed() {
    let forwarded = forwarded_downgrade_body("bc-prop-datasetid", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[("speed", json!({ "type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:ds:1" }))],
        )
    })
    .await;
    assert!(
        forwarded["speed"].get("datasetId").is_none(),
        "a property datasetId is removed: {}",
        forwarded["speed"]
    );
}

/// A property's `expiresAt` sub-attribute is removed for an older source (Table
/// 4.3.6.8-2). Confirms REQ-FW-62.
#[tokio::test]
async fn distop_entity_bc_property_sub_attr_expires_at_removed() {
    let forwarded = forwarded_downgrade_body("bc-prop-expiresat", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[("speed", json!({ "type": "Property", "value": 10, "expiresAt": "2099-01-01T00:00:00Z" }))],
        )
    })
    .await;
    assert!(
        forwarded["speed"].get("expiresAt").is_none(),
        "a property expiresAt is removed: {}",
        forwarded["speed"]
    );
}

/// A property's `observedAt` sub-attribute is removed for an older source (Table
/// 4.3.6.8-2). Confirms REQ-FW-62.
#[tokio::test]
async fn distop_entity_bc_property_sub_attr_observed_at_removed() {
    let forwarded = forwarded_downgrade_body("bc-prop-observedat", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[("speed", json!({ "type": "Property", "value": 10, "observedAt": "2020-01-01T00:00:00Z" }))],
        )
    })
    .await;
    assert!(
        forwarded["speed"].get("observedAt").is_none(),
        "a property observedAt is removed: {}",
        forwarded["speed"]
    );
}

/// A property's `unitCode` sub-attribute is removed for an older source (Table
/// 4.3.6.8-2). Confirms REQ-FW-62.
#[tokio::test]
async fn distop_entity_bc_property_sub_attr_unit_code_removed() {
    let forwarded = forwarded_downgrade_body("bc-prop-unitcode", "1.0", |id| {
        entity(id, "Vehicle", &[("speed", json!({ "type": "Property", "value": 10, "unitCode": "KMH" }))])
    })
    .await;
    assert!(
        forwarded["speed"].get("unitCode").is_none(),
        "a property unitCode is removed: {}",
        forwarded["speed"]
    );
}

/// A property's `valueType` sub-attribute is removed for an older source (Table
/// 4.3.6.8-2). Confirms REQ-FW-62.
#[tokio::test]
async fn distop_entity_bc_property_sub_attr_value_type_removed() {
    let forwarded = forwarded_downgrade_body("bc-prop-valuetype", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "speed",
                json!({ "type": "Property", "value": 10, "valueType": "http://www.w3.org/2001/XMLSchema#integer" }),
            )],
        )
    })
    .await;
    assert!(
        forwarded["speed"].get("valueType").is_none(),
        "a property valueType is removed: {}",
        forwarded["speed"]
    );
}

/// A relationship's `datasetId` sub-attribute is removed for an older source
/// (Table 4.3.6.8-3). Confirms REQ-FW-63.
#[tokio::test]
async fn distop_entity_bc_relationship_sub_attr_dataset_id_removed() {
    let forwarded = forwarded_downgrade_body("bc-rel-datasetid", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "parkedIn",
                json!({ "type": "Relationship", "object": "urn:ngsi-ld:P:1", "datasetId": "urn:ngsi-ld:ds:1" }),
            )],
        )
    })
    .await;
    assert!(
        forwarded["parkedIn"].get("datasetId").is_none(),
        "a relationship datasetId is removed: {}",
        forwarded["parkedIn"]
    );
}

/// A relationship's `expiresAt` sub-attribute is removed for an older source
/// (Table 4.3.6.8-3). Confirms REQ-FW-63.
#[tokio::test]
async fn distop_entity_bc_relationship_sub_attr_expires_at_removed() {
    let forwarded = forwarded_downgrade_body("bc-rel-expiresat", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "parkedIn",
                json!({ "type": "Relationship", "object": "urn:ngsi-ld:P:1", "expiresAt": "2099-01-01T00:00:00Z" }),
            )],
        )
    })
    .await;
    assert!(
        forwarded["parkedIn"].get("expiresAt").is_none(),
        "a relationship expiresAt is removed: {}",
        forwarded["parkedIn"]
    );
}

/// A relationship's `objectType` sub-attribute is removed for an older source
/// (Table 4.3.6.8-3). Confirms REQ-FW-63.
#[tokio::test]
async fn distop_entity_bc_relationship_sub_attr_object_type_removed() {
    let forwarded = forwarded_downgrade_body("bc-rel-objecttype", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "parkedIn",
                json!({ "type": "Relationship", "object": "urn:ngsi-ld:P:1", "objectType": "OffStreetParking" }),
            )],
        )
    })
    .await;
    assert!(
        forwarded["parkedIn"].get("objectType").is_none(),
        "a relationship objectType is removed: {}",
        forwarded["parkedIn"]
    );
}

/// A relationship's `observedAt` sub-attribute is removed for an older source
/// (Table 4.3.6.8-3). Confirms REQ-FW-63.
#[tokio::test]
async fn distop_entity_bc_relationship_sub_attr_observed_at_removed() {
    let forwarded = forwarded_downgrade_body("bc-rel-observedat", "1.0", |id| {
        entity(
            id,
            "Vehicle",
            &[(
                "parkedIn",
                json!({ "type": "Relationship", "object": "urn:ngsi-ld:P:1", "observedAt": "2020-01-01T00:00:00Z" }),
            )],
        )
    })
    .await;
    assert!(
        forwarded["parkedIn"].get("observedAt").is_none(),
        "a relationship observedAt is removed: {}",
        forwarded["parkedIn"]
    );
}
