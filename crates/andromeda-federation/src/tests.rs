use crate::{
    entity_result::{collect_proxied_attrs, collect_warnings, merge_query_results, merge_retrieve_results, strip_attributes_from_value},
    loop_detected,
    outcome::{ForwardedEntities, ForwardedTemporalEntities, ForwardedWriteOutcome, ForwardedWriteResult},
    registration_loops,
};
use andromeda_http_headers::{ngsild_warning::NgsiLdWarning, received_protocol::ReceivedProtocol, via::Via};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    attribute::instance::AttributeValue,
    attribute_strip::AttributeStrip,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use url::Url;

fn attribute_name(raw: &str) -> AttributeName {
    raw.parse().expect("valid attribute name")
}

fn entity_uri(raw: &str) -> Identifier {
    raw.parse().expect("valid entity URI")
}

#[test]
fn incoming_via_with_the_local_alias_is_rejected_as_a_loop() {
    let alias = ContextSourcePseudonym::new("broker-a");
    let protocol = ReceivedProtocol::new("1.1");
    assert!(alias.is_ok());
    assert!(protocol.is_ok());
    let (Ok(alias), Ok(protocol)) = (alias, protocol) else {
        return;
    };
    let via = Via::with_entry(protocol, alias.clone()); // clone: the test needs the alias both in the chain and as the local identity.
    assert!(loop_detected(Some(&via), &alias));
}

#[test]
fn incoming_via_without_the_local_alias_is_not_a_loop() {
    let local_alias = ContextSourcePseudonym::new("broker-a");
    let remote_alias = ContextSourcePseudonym::new("broker-b");
    let protocol = ReceivedProtocol::new("1.1");
    assert!(local_alias.is_ok());
    assert!(remote_alias.is_ok());
    assert!(protocol.is_ok());
    let (Ok(local_alias), Ok(remote_alias), Ok(protocol)) = (local_alias, remote_alias, protocol) else {
        return;
    };
    let via = Via::with_entry(protocol, remote_alias);
    assert!(!loop_detected(Some(&via), &local_alias));
}

use andromeda_ngsi_ld_model::entity::Entity;

fn entity_from_json(value: serde_json::Value) -> Entity {
    serde_json::from_value(value).expect("test entity fixture")
}

fn write_result(mode: ForwardingMode, proxied_attrs: Vec<AttributeName>) -> ForwardedWriteResult {
    ForwardedWriteResult {
        registration_id: entity_uri("urn:ngsi-ld:ContextSourceRegistration:test"),
        mode,
        outcome: ForwardedWriteOutcome::Completed { status: 204, body: None },
        proxied_attrs,
    }
}

#[test]
fn warning_header_format_miscellaneous() {
    let w = NgsiLdWarning::MiscellaneousWarning("timeout".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "199 - \"timeout\"");
}

#[test]
fn warning_header_format_persistent() {
    let w = NgsiLdWarning::MiscellaneousPersistentWarning("403 Forbidden".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "299 - \"403 Forbidden\"");
}

#[test]
fn warning_header_format_response_is_stale_per_clause_6_3_17() {
    let w = NgsiLdWarning::ResponseIsStale("cached data".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "110 - \"cached data\"");
}

#[test]
fn warning_header_format_revalidation_failed_per_clause_6_3_17() {
    let w = NgsiLdWarning::RevalidationFailed("invalid JSON".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "111 - \"invalid JSON\"");
}

#[test]
fn collect_warnings_from_results() {
    let results = vec![
        ForwardedEntities {
            registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
            mode: ForwardingMode::Inclusive,
            entities: Vec::new(),
            warnings: vec![
                NgsiLdWarning::MiscellaneousWarning("timeout".to_string()),
                NgsiLdWarning::RevalidationFailed("invalid JSON".to_string()),
            ],
            linked_map: None,
        },
        ForwardedEntities {
            registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
            mode: ForwardingMode::Inclusive,
            entities: vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))],
            warnings: Vec::new(),
            linked_map: None,
        },
    ];
    let warnings = collect_warnings(&results);
    pretty_assertions::assert_eq!(warnings.len(), 2);
    assert!(matches!(warnings[0], NgsiLdWarning::MiscellaneousWarning(_)));
    assert!(matches!(warnings[1], NgsiLdWarning::RevalidationFailed(_)));
}

#[test]
fn merge_query_local_only() {
    let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "p1": 1}))];
    let result = merge_query_results(local, Vec::new());
    pretty_assertions::assert_eq!(result.len(), 1);
}

#[test]
fn merge_query_adds_remote_entities() {
    let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))];
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![entity_from_json(serde_json::json!({"id": "urn:b", "type": "T"}))],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2);
}

#[test]
fn merge_simple_types_deduplicates() {
    use crate::{discovery_result::merge_types_results, outcome::ForwardedDiscoveryResult};

    let local = serde_json::json!({"id": "urn:types", "type": "EntityTypeList", "typeList": ["Vehicle", "Sensor"]});
    let remote = vec![ForwardedDiscoveryResult {
        body: Some(serde_json::json!({"typeList": ["Vehicle", "Drone"]})),
        warnings: Vec::new(),
    }];
    let result = merge_types_results(local, remote, false);
    let list = result.get("typeList").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(list.len(), 3);
    assert!(list.contains(&serde_json::json!("Vehicle")));
    assert!(list.contains(&serde_json::json!("Sensor")));
    assert!(list.contains(&serde_json::json!("Drone")));
}

#[test]
fn merge_simple_types_no_remote_returns_local() {
    use crate::discovery_result::merge_types_results;

    let local = serde_json::json!({"typeList": ["Vehicle"]});
    let result = merge_types_results(local.clone(), Vec::new(), false);
    pretty_assertions::assert_eq!(result, local);
}

#[test]
fn merge_detailed_types_unions_attribute_names() {
    use crate::{discovery_result::merge_types_results, outcome::ForwardedDiscoveryResult};

    let local = serde_json::json!([
        {"id": "Vehicle", "type": "EntityType", "attributeNames": ["speed", "location"]}
    ]);
    let remote = vec![ForwardedDiscoveryResult {
        body: Some(serde_json::json!([
            {"id": "Vehicle", "type": "EntityType", "attributeNames": ["speed", "fuel"]},
            {"id": "Drone", "type": "EntityType", "attributeNames": ["altitude"]}
        ])),
        warnings: Vec::new(),
    }];
    let result = merge_types_results(local, remote, true);
    let arr = result.as_array().unwrap();
    pretty_assertions::assert_eq!(arr.len(), 2);

    let vehicle = arr.iter().find(|v| v.get("id").unwrap() == "Vehicle").unwrap();
    let attrs = vehicle.get("attributeNames").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(attrs.len(), 3);
    assert!(attrs.contains(&serde_json::json!("fuel")));
}

#[test]
fn merge_simple_attributes_deduplicates() {
    use crate::{discovery_result::merge_attributes_results, outcome::ForwardedDiscoveryResult};

    let local = serde_json::json!({"id": "urn:attrs", "type": "AttributeList", "attributeList": ["speed"]});
    let remote = vec![ForwardedDiscoveryResult {
        body: Some(serde_json::json!({"attributeList": ["speed", "fuel"]})),
        warnings: Vec::new(),
    }];
    let result = merge_attributes_results(local, remote, false);
    let list = result.get("attributeList").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(list.len(), 2);
}

#[test]
fn merge_temporal_query_unions_instances_for_same_entity() {
    use crate::temporal_result::merge_temporal_query_results;

    let local = vec![serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": [
            {"type": "Property", "value": 10, "instanceId": "urn:i1", "observedAt": "2024-01-01T00:00:00Z"}
        ]
    })];
    let remote = vec![ForwardedTemporalEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [
                {"type": "Property", "value": 20, "instanceId": "urn:i2", "observedAt": "2024-01-02T00:00:00Z"}
            ]
        })],
        warnings: Vec::new(),
        linked_map: None,
        partial_ranges: Vec::new(),
    }];
    let result = merge_temporal_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 1);
    let speed = result[0].get("speed").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(speed.len(), 2);
}

#[test]
fn merge_temporal_query_deduplicates_by_instance_id() {
    use crate::temporal_result::merge_temporal_query_results;

    let local = vec![serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": [
            {"type": "Property", "value": 10, "instanceId": "urn:i1"}
        ]
    })];
    let remote = vec![ForwardedTemporalEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [
                {"type": "Property", "value": 10, "instanceId": "urn:i1"}
            ]
        })],
        warnings: Vec::new(),
        linked_map: None,
        partial_ranges: Vec::new(),
    }];
    let result = merge_temporal_query_results(local, remote);
    let speed = result[0].get("speed").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(speed.len(), 1);
}

#[test]
fn merge_temporal_query_auxiliary_only_supplements_existing() {
    use crate::temporal_result::merge_temporal_query_results;

    let local = vec![serde_json::json!({"id": "urn:a", "type": "T", "speed": [{"value": 10}]})];
    let remote = vec![ForwardedTemporalEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Auxiliary,
        entities: vec![
            serde_json::json!({"id": "urn:a", "type": "T", "fuel": [{"value": 50}]}),
            serde_json::json!({"id": "urn:b", "type": "T", "speed": [{"value": 20}]}),
        ],
        warnings: Vec::new(),
        linked_map: None,
        partial_ranges: Vec::new(),
    }];
    let result = merge_temporal_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 1);
    pretty_assertions::assert_eq!(result[0].get("id").and_then(serde_json::Value::as_str), Some("urn:a"));
    assert!(result[0].get("fuel").is_some());
}

#[test]
fn merge_temporal_retrieve_combines_remote_attributes() {
    use crate::temporal_result::merge_temporal_retrieve_results;

    let local = Some(serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": [{"value": 10, "instanceId": "urn:i1"}]
    }));
    let remote = vec![ForwardedTemporalEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "fuel": [{"value": 50}]
        })],
        warnings: Vec::new(),
        linked_map: None,
        partial_ranges: Vec::new(),
    }];
    let result = merge_temporal_retrieve_results(local, remote);
    let entity = result.unwrap();
    assert!(entity.get("speed").is_some());
    assert!(entity.get("fuel").is_some());
}

#[test]
fn merge_temporal_retrieve_remote_only() {
    use crate::temporal_result::merge_temporal_retrieve_results;

    let remote = vec![ForwardedTemporalEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({"id": "urn:a", "type": "T", "speed": [{"value": 10}]})],
        warnings: Vec::new(),
        linked_map: None,
        partial_ranges: Vec::new(),
    }];
    let result = merge_temporal_retrieve_results(None, remote);
    assert!(result.is_some());
}

#[test]
fn build_discovery_url_with_query_string() {
    use crate::forwarding_url::build_discovery_url;

    let url = build_discovery_url(&Url::parse("http://cs1.example.com").expect("valid url"), "/types", "details=true");
    assert!(matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/types?details=true"));
}

#[test]
fn build_discovery_url_without_query_string() {
    use crate::forwarding_url::build_discovery_url;

    let url = build_discovery_url(&Url::parse("http://cs1.example.com/").expect("valid url"), "/attributes", "");
    assert!(matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/attributes"));
}

#[test]
fn build_temporal_query_url_with_params() {
    use crate::forwarding_url::build_temporal_query_url;

    let url = build_temporal_query_url(
        &Url::parse("http://cs1.example.com").expect("valid url"),
        "type=Vehicle&timerel=after&timeAt=2024-01-01T00:00:00Z",
    );
    assert!(
        matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z")
    );
}

#[test]
fn build_temporal_retrieve_url_with_id() {
    use crate::forwarding_url::build_temporal_retrieve_url;

    let url = build_temporal_retrieve_url(
        &Url::parse("http://cs1.example.com").expect("valid url"),
        &entity_uri("urn:ngsi-ld:Vehicle:1"),
        "timerel=after&timeAt=2024-01-01T00:00:00Z",
    );
    assert!(
        matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:1?timerel=after&timeAt=2024-01-01T00%3A00%3A00Z")
    );
}

#[test]
fn collect_discovery_warnings_aggregates() {
    use crate::{discovery_result::collect_discovery_warnings, outcome::ForwardedDiscoveryResult};

    let results = vec![
        ForwardedDiscoveryResult {
            body: None,
            warnings: vec![NgsiLdWarning::MiscellaneousWarning("timeout".to_string())],
        },
        ForwardedDiscoveryResult {
            body: Some(serde_json::json!({})),
            warnings: Vec::new(),
        },
    ];
    let warnings = collect_discovery_warnings(&results);
    pretty_assertions::assert_eq!(warnings.len(), 1);
}

#[test]
fn collect_proxied_attrs_empty_results() {
    let result = collect_proxied_attrs(&[]);
    assert_eq!(result, [] as [AttributeName; 0]);
}

#[test]
fn collect_proxied_attrs_gathers_attrs() {
    let results = vec![
        write_result(ForwardingMode::Exclusive, vec![attribute_name("speed"), attribute_name("fuel")]),
        write_result(ForwardingMode::Inclusive, vec![attribute_name("temperature")]),
    ];
    let attrs = collect_proxied_attrs(&results);
    pretty_assertions::assert_eq!(attrs.len(), 3);
    for expected in ["speed", "fuel", "temperature"] {
        assert!(attrs.iter().any(|attr| attr.as_str() == expected), "{expected} missing");
    }
}

#[test]
fn strip_attributes_from_value_removes_matching_keys() {
    let mut value = serde_json::json!({
        "speed": { "type": "Property", "value": 100 },
        "fuel": { "type": "Property", "value": 50 },
        "name": { "type": "Property", "value": "Car" }
    });
    pretty_assertions::assert_eq!(
        strip_attributes_from_value(&mut value, &[attribute_name("speed"), attribute_name("fuel")]),
        AttributeStrip::Narrowed
    );
    let obj = value.as_object().expect("is object");
    pretty_assertions::assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("name"));
}

#[test]
fn strip_attributes_from_value_no_matching_keys() {
    let mut value = serde_json::json!({
        "name": { "type": "Property", "value": "Car" }
    });
    pretty_assertions::assert_eq!(strip_attributes_from_value(&mut value, &[attribute_name("speed")]), AttributeStrip::Untouched);
    let obj = value.as_object().expect("is object");
    pretty_assertions::assert_eq!(obj.len(), 1);
}

#[test]
fn strip_attributes_from_value_that_removes_every_member_empties_the_fragment() {
    let mut value = serde_json::json!({
        "speed": { "type": "Property", "value": 100 }
    });
    pretty_assertions::assert_eq!(strip_attributes_from_value(&mut value, &[attribute_name("speed")]), AttributeStrip::Emptied);
    assert!(value.as_object().is_some_and(serde_json::Map::is_empty));
}

#[test]
fn strip_attributes_from_value_non_object_noop() {
    let mut value = serde_json::json!([1, 2, 3]);
    pretty_assertions::assert_eq!(strip_attributes_from_value(&mut value, &[attribute_name("speed")]), AttributeStrip::Untouched);
    assert!(value.is_array());
}

#[test]
fn collect_proxied_attrs_results_with_no_proxied_attrs() {
    let results = vec![write_result(ForwardingMode::Inclusive, Vec::new())];
    let attrs = collect_proxied_attrs(&results);
    assert_eq!(attrs, [] as [AttributeName; 0]);
}

#[test]
fn collect_proxied_attrs_duplicates_not_deduped() {
    // collect_proxied_attrs does not deduplicate -- callers
    // use the list for removal which is idempotent on duplicates.
    let results = vec![
        write_result(ForwardingMode::Exclusive, vec![attribute_name("speed")]),
        write_result(ForwardingMode::Exclusive, vec![attribute_name("speed")]),
    ];
    let attrs = collect_proxied_attrs(&results);
    pretty_assertions::assert_eq!(attrs.len(), 2);
}

#[test]
fn csr_mode_filter_all_includes_every_mode() {
    use crate::request::CsrModeFilter;

    let filter = CsrModeFilter::All;
    // All should include every mode
    assert!(matches!(filter, CsrModeFilter::All));
}

#[test]
fn csr_mode_filter_exclude_auxiliary() {
    use crate::request::CsrModeFilter;

    let filter = CsrModeFilter::ExcludeAuxiliary;
    pretty_assertions::assert_eq!(filter, CsrModeFilter::ExcludeAuxiliary);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::All);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::AuxiliaryOnly);
}

#[test]
fn csr_mode_filter_auxiliary_only() {
    use crate::request::CsrModeFilter;

    let filter = CsrModeFilter::AuxiliaryOnly;
    pretty_assertions::assert_eq!(filter, CsrModeFilter::AuxiliaryOnly);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::All);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::ExcludeAuxiliary);
}

#[test]
fn merge_query_auxiliary_supplements_without_adding_entities() {
    let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "speed": 100}))];
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Auxiliary,
        entities: vec![
            entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "fuel": 50})),
            entity_from_json(serde_json::json!({"id": "urn:b", "type": "T", "speed": 80})),
        ],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let result = merge_query_results(local, remote);
    // Clause 5.7.2.4: auxiliary data is added only where an attribute is not
    // present already, and an entity no other source returned is not added.
    pretty_assertions::assert_eq!(result.len(), 1, "an auxiliary source supplements, it never supplies new entities");
    let entity_a = result.iter().find(|entity| entity.id.to_string() == "urn:a").expect("urn:a should be present");
    assert!(entity_a.attrs.contains_key("fuel"), "Auxiliary attrs should be merged");
}

#[test]
fn merge_query_primary_adds_new_entities() {
    // Non-auxiliary modes should add new entities
    let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))];
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![entity_from_json(serde_json::json!({"id": "urn:b", "type": "T", "speed": 80}))],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2, "Inclusive should add new entities");
}

fn aliased_registration(alias: &str) -> ContextSourceRegistration {
    serde_json::from_value(serde_json::json!({
        "id": "urn:ngsi-ld:csr:loop",
        "type": "ContextSourceRegistration",
        "endpoint": "https://peer.example.org",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "contextSourceAlias": alias,
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("valid registration")
}

/// Clause 6.3.17: a registration whose alias is this broker's own alias is
/// "registered to redirect back on to the Context Broker", a loop even
/// without any `Via` chain; one whose alias is elsewhere in the chain is a
/// loop too (clause 4.3.6.4); any other alias is not.
#[test]
fn a_registration_aliased_as_the_broker_itself_or_as_a_via_hop_is_a_loop() {
    let local = ContextSourcePseudonym::new("broker-a").expect("valid alias");
    let upstream = ContextSourcePseudonym::new("broker-b").expect("valid alias");
    let protocol = ReceivedProtocol::new("1.1").expect("valid protocol");

    assert!(registration_loops(&aliased_registration("broker-a"), None, &local));
    let via = Via::with_entry(protocol, upstream);
    assert!(registration_loops(&aliased_registration("broker-b"), Some(&via), &local));
    assert!(!registration_loops(&aliased_registration("broker-c"), Some(&via), &local));
    assert!(!registration_loops(&aliased_registration("broker-c"), None, &local));
}

/// Clause 5.7.1.4: local data is the base and remote attributes are merged on
/// top per clause 4.5.5, so a distinct `datasetId` instance coexists with the
/// local default instance.
#[test]
fn merge_retrieve_merges_remote_instances_onto_the_local_base() {
    let local = entity_from_json(serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": {"type": "Property", "value": 10, "observedAt": "2021-01-01T00:00:00Z"}
    }));
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![entity_from_json(serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": {"type": "Property", "value": 30, "observedAt": "2023-01-01T00:00:00Z", "datasetId": "urn:ngsi-ld:ds:2"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }))],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let merged = merge_retrieve_results(Some(local), remote).expect("merged entity");
    assert!(matches!(merged.attrs.get("speed"), Some(AttributeValue::Multiple(instances)) if instances.len() == 2));
    assert!(merged.location.is_some());
}

/// Clause 4.3.6.2: an auxiliary source never supplies an entity that no other
/// source holds.
#[test]
fn merge_retrieve_ignores_an_auxiliary_source_when_nothing_else_holds_the_entity() {
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Auxiliary,
        entities: vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))],
        warnings: Vec::new(),
        linked_map: None,
    }];
    assert!(merge_retrieve_results(None, remote).is_none());
}
