use crate::service::csource_forwarding::{
    entity_result::{collect_forwarded_exclusive_attrs, collect_warnings, merge_query_results, strip_exclusive_attrs_from_value},
    forwarding_url::build_forwarding_query_string,
    outcome::{ForwardedEntities, ForwardedTemporalEntities, ForwardedWriteOutcome, ForwardedWriteResult},
};
use andromeda_ngsi_ld_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use url::Url;

fn attribute_name(raw: &str) -> AttributeName {
    raw.parse().expect("valid attribute name")
}

fn entity_from_json(value: serde_json::Value) -> andromeda_ngsi_ld_model::entity::Entity {
    serde_json::from_value(value).expect("test entity fixture")
}

fn write_result(mode: ForwardingMode, exclusive_attrs: Vec<String>) -> ForwardedWriteResult {
    ForwardedWriteResult {
        mode,
        outcome: ForwardedWriteOutcome::Completed { status: 204, body: None },
        exclusive_attrs,
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
            mode: ForwardingMode::Inclusive,
            entities: Vec::new(),
            warnings: vec![
                NgsiLdWarning::MiscellaneousWarning("timeout".to_string()),
                NgsiLdWarning::RevalidationFailed("invalid JSON".to_string()),
            ],
        },
        ForwardedEntities {
            mode: ForwardingMode::Inclusive,
            entities: vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))],
            warnings: Vec::new(),
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
        mode: ForwardingMode::Inclusive,
        entities: vec![entity_from_json(serde_json::json!({"id": "urn:b", "type": "T"}))],
        warnings: Vec::new(),
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2);
}

#[test]
fn merge_simple_types_deduplicates() {
    use crate::service::csource_forwarding::{discovery_result::merge_types_results, outcome::ForwardedDiscoveryResult};

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
    use crate::service::csource_forwarding::discovery_result::merge_types_results;

    let local = serde_json::json!({"typeList": ["Vehicle"]});
    let result = merge_types_results(local.clone(), Vec::new(), false);
    pretty_assertions::assert_eq!(result, local);
}

#[test]
fn merge_detailed_types_unions_attribute_names() {
    use crate::service::csource_forwarding::{discovery_result::merge_types_results, outcome::ForwardedDiscoveryResult};

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
    use crate::service::csource_forwarding::{discovery_result::merge_attributes_results, outcome::ForwardedDiscoveryResult};

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
    use crate::service::csource_forwarding::temporal_result::merge_temporal_query_results;

    let local = vec![serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": [
            {"type": "Property", "value": 10, "instanceId": "urn:i1", "observedAt": "2024-01-01T00:00:00Z"}
        ]
    })];
    let remote = vec![ForwardedTemporalEntities {
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [
                {"type": "Property", "value": 20, "instanceId": "urn:i2", "observedAt": "2024-01-02T00:00:00Z"}
            ]
        })],
        warnings: Vec::new(),
    }];
    let result = merge_temporal_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 1);
    let speed = result[0].get("speed").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(speed.len(), 2);
}

#[test]
fn merge_temporal_query_deduplicates_by_instance_id() {
    use crate::service::csource_forwarding::temporal_result::merge_temporal_query_results;

    let local = vec![serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": [
            {"type": "Property", "value": 10, "instanceId": "urn:i1"}
        ]
    })];
    let remote = vec![ForwardedTemporalEntities {
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "speed": [
                {"type": "Property", "value": 10, "instanceId": "urn:i1"}
            ]
        })],
        warnings: Vec::new(),
    }];
    let result = merge_temporal_query_results(local, remote);
    let speed = result[0].get("speed").unwrap().as_array().unwrap();
    pretty_assertions::assert_eq!(speed.len(), 1);
}

#[test]
fn merge_temporal_query_auxiliary_only_supplements_existing() {
    use crate::service::csource_forwarding::temporal_result::merge_temporal_query_results;

    let local = vec![serde_json::json!({"id": "urn:a", "type": "T", "speed": [{"value": 10}]})];
    let remote = vec![ForwardedTemporalEntities {
        mode: ForwardingMode::Auxiliary,
        entities: vec![
            serde_json::json!({"id": "urn:a", "type": "T", "fuel": [{"value": 50}]}),
            serde_json::json!({"id": "urn:b", "type": "T", "speed": [{"value": 20}]}),
        ],
        warnings: Vec::new(),
    }];
    let result = merge_temporal_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 1);
    pretty_assertions::assert_eq!(result[0].get("id").and_then(serde_json::Value::as_str), Some("urn:a"));
    assert!(result[0].get("fuel").is_some());
}

#[test]
fn merge_temporal_retrieve_combines_remote_attributes() {
    use crate::service::csource_forwarding::temporal_result::merge_temporal_retrieve_results;

    let local = Some(serde_json::json!({
        "id": "urn:a",
        "type": "T",
        "speed": [{"value": 10, "instanceId": "urn:i1"}]
    }));
    let remote = vec![ForwardedTemporalEntities {
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({
            "id": "urn:a",
            "type": "T",
            "fuel": [{"value": 50}]
        })],
        warnings: Vec::new(),
    }];
    let result = merge_temporal_retrieve_results(local, remote);
    let entity = result.unwrap();
    assert!(entity.get("speed").is_some());
    assert!(entity.get("fuel").is_some());
}

#[test]
fn merge_temporal_retrieve_remote_only() {
    use crate::service::csource_forwarding::temporal_result::merge_temporal_retrieve_results;

    let remote = vec![ForwardedTemporalEntities {
        mode: ForwardingMode::Inclusive,
        entities: vec![serde_json::json!({"id": "urn:a", "type": "T", "speed": [{"value": 10}]})],
        warnings: Vec::new(),
    }];
    let result = merge_temporal_retrieve_results(None, remote);
    assert!(result.is_some());
}

#[test]
fn build_discovery_url_with_query_string() {
    use crate::service::csource_forwarding::forwarding_url::build_discovery_url;

    let url = build_discovery_url(&Url::parse("http://cs1.example.com").expect("valid url"), "/types", "details=true");
    assert!(matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/types?details=true"));
}

#[test]
fn build_discovery_url_without_query_string() {
    use crate::service::csource_forwarding::forwarding_url::build_discovery_url;

    let url = build_discovery_url(&Url::parse("http://cs1.example.com/").expect("valid url"), "/attributes", "");
    assert!(matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/attributes"));
}

#[test]
fn build_temporal_query_url_with_params() {
    use crate::service::csource_forwarding::forwarding_url::build_temporal_query_url;

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
    use crate::service::csource_forwarding::forwarding_url::build_temporal_retrieve_url;

    let url = build_temporal_retrieve_url(
        &Url::parse("http://cs1.example.com").expect("valid url"),
        "urn:ngsi-ld:Vehicle:1",
        "timerel=after&timeAt=2024-01-01T00:00:00Z",
    );
    assert!(
        matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:1?timerel=after&timeAt=2024-01-01T00%3A00%3A00Z")
    );
}

#[test]
fn collect_discovery_warnings_aggregates() {
    use crate::service::csource_forwarding::{discovery_result::collect_discovery_warnings, outcome::ForwardedDiscoveryResult};

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
fn collect_forwarded_exclusive_attrs_empty_results() {
    let result = collect_forwarded_exclusive_attrs(&[]);
    assert!(result.is_empty());
}

#[test]
fn collect_forwarded_exclusive_attrs_gathers_attrs() {
    let results = vec![
        write_result(ForwardingMode::Exclusive, vec!["speed".to_string(), "fuel".to_string()]),
        write_result(ForwardingMode::Inclusive, vec!["temperature".to_string()]),
    ];
    let attrs = collect_forwarded_exclusive_attrs(&results);
    pretty_assertions::assert_eq!(attrs.len(), 3);
    for expected in ["speed", "fuel", "temperature"] {
        assert!(attrs.iter().any(|attr| attr.as_str() == expected), "{expected} missing");
    }
}

#[test]
fn strip_exclusive_attrs_from_value_removes_matching_keys() {
    let mut value = serde_json::json!({
        "speed": { "type": "Property", "value": 100 },
        "fuel": { "type": "Property", "value": 50 },
        "name": { "type": "Property", "value": "Car" }
    });
    strip_exclusive_attrs_from_value(&mut value, &[attribute_name("speed"), attribute_name("fuel")]);
    let obj = value.as_object().expect("is object");
    pretty_assertions::assert_eq!(obj.len(), 1);
    assert!(obj.contains_key("name"));
}

#[test]
fn strip_exclusive_attrs_from_value_no_matching_keys() {
    let mut value = serde_json::json!({
        "name": { "type": "Property", "value": "Car" }
    });
    strip_exclusive_attrs_from_value(&mut value, &[attribute_name("speed")]);
    let obj = value.as_object().expect("is object");
    pretty_assertions::assert_eq!(obj.len(), 1);
}

#[test]
fn strip_exclusive_attrs_from_value_non_object_noop() {
    let mut value = serde_json::json!([1, 2, 3]);
    strip_exclusive_attrs_from_value(&mut value, &[attribute_name("speed")]);
    assert!(value.is_array());
}

#[test]
fn collect_forwarded_exclusive_attrs_results_with_no_exclusive_attrs() {
    let results = vec![write_result(ForwardingMode::Inclusive, Vec::new())];
    let attrs = collect_forwarded_exclusive_attrs(&results);
    assert!(attrs.is_empty());
}

#[test]
fn collect_forwarded_exclusive_attrs_duplicates_not_deduped() {
    // collect_forwarded_exclusive_attrs does not deduplicate -- callers
    // use the list for removal which is idempotent on duplicates.
    let results = vec![
        write_result(ForwardingMode::Exclusive, vec!["speed".to_string()]),
        write_result(ForwardingMode::Exclusive, vec!["speed".to_string()]),
    ];
    let attrs = collect_forwarded_exclusive_attrs(&results);
    pretty_assertions::assert_eq!(attrs.len(), 2);
}

#[test]
fn csr_mode_filter_all_includes_every_mode() {
    use crate::service::csource_forwarding::request::CsrModeFilter;

    let filter = CsrModeFilter::All;
    // All should include every mode
    assert!(matches!(filter, CsrModeFilter::All));
}

#[test]
fn csr_mode_filter_exclude_auxiliary() {
    use crate::service::csource_forwarding::request::CsrModeFilter;

    let filter = CsrModeFilter::ExcludeAuxiliary;
    pretty_assertions::assert_eq!(filter, CsrModeFilter::ExcludeAuxiliary);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::All);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::AuxiliaryOnly);
}

#[test]
fn csr_mode_filter_auxiliary_only() {
    use crate::service::csource_forwarding::request::CsrModeFilter;

    let filter = CsrModeFilter::AuxiliaryOnly;
    pretty_assertions::assert_eq!(filter, CsrModeFilter::AuxiliaryOnly);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::All);
    pretty_assertions::assert_ne!(filter, CsrModeFilter::ExcludeAuxiliary);
}

#[test]
fn merge_query_auxiliary_adds_and_merges() {
    let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "speed": 100}))];
    let remote = vec![ForwardedEntities {
        mode: ForwardingMode::Auxiliary,
        entities: vec![
            entity_from_json(serde_json::json!({"id": "urn:a", "type": "T", "fuel": 50})),
            entity_from_json(serde_json::json!({"id": "urn:b", "type": "T", "speed": 80})),
        ],
        warnings: Vec::new(),
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2, "Auxiliary should add remote entities in query merge");
    let entity_a = result.iter().find(|entity| entity.id.to_string() == "urn:a").expect("urn:a should be present");
    assert!(entity_a.attrs.contains_key("fuel"), "Auxiliary attrs should be merged");
}

#[test]
fn merge_query_primary_adds_new_entities() {
    // Non-auxiliary modes should add new entities
    let local = vec![entity_from_json(serde_json::json!({"id": "urn:a", "type": "T"}))];
    let remote = vec![ForwardedEntities {
        mode: ForwardingMode::Inclusive,
        entities: vec![entity_from_json(serde_json::json!({"id": "urn:b", "type": "T", "speed": 80}))],
        warnings: Vec::new(),
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2, "Inclusive should add new entities");
}

#[test]
fn forwarding_query_string_renders_typed_ids_without_reparsing() {
    let ids = vec![
        "urn:ngsi-ld:Entity:a".parse::<Uri>().expect("valid URI"),
        "urn:ngsi-ld:Entity:b".parse::<Uri>().expect("valid URI"),
    ];
    pretty_assertions::assert_eq!(
        build_forwarding_query_string(Some("Vehicle"), Some(&ids)),
        "id=urn%3Angsi-ld%3AEntity%3Aa%2Curn%3Angsi-ld%3AEntity%3Ab&type=Vehicle"
    );
}
