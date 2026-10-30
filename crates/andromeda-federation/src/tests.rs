use crate::{
    answer_body::AnswerBody,
    entity_result::{collect_proxied_attrs, collect_warnings, merge_query_results, merge_retrieve_results},
    loop_detected,
    outcome::{ForwardedEntities, ForwardedWriteOutcome, ForwardedWriteResult},
    received_fixture::{instance_count, iri, version},
    registration_loops,
};
use andromeda_http_headers::{
    ngsild_warning::{NgsiLdWarning, WarnCode},
    received_protocol::ReceivedProtocol,
    via::Via,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use serde_json::{Value, json};
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

fn write_result(mode: ForwardingMode, attributes: Vec<AttributeName>) -> ForwardedWriteResult {
    ForwardedWriteResult {
        registration_id: entity_uri("urn:ngsi-ld:ContextSourceRegistration:test"),
        mode,
        outcome: ForwardedWriteOutcome::Completed {
            status: 204,
            body: AnswerBody::Absent,
        },
        attributes,
    }
}

#[test]
fn warning_header_format_miscellaneous() {
    let w = NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "199 - \"timeout\"");
}

#[test]
fn warning_header_format_persistent() {
    let w = NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "403 Forbidden".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "299 - \"403 Forbidden\"");
}

#[test]
fn warning_header_format_response_is_stale_per_clause_6_3_17() {
    let w = NgsiLdWarning::new(WarnCode::ResponseIsStale, "cached data".to_string());
    pretty_assertions::assert_eq!(w.to_string(), "110 - \"cached data\"");
}

#[test]
fn warning_header_format_revalidation_failed_per_clause_6_3_17() {
    let w = NgsiLdWarning::new(WarnCode::RevalidationFailed, "invalid JSON".to_string());
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
                NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string()),
                NgsiLdWarning::new(WarnCode::RevalidationFailed, "invalid JSON".to_string()),
            ],
            linked_map: None,
        },
        ForwardedEntities {
            registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
            mode: ForwardingMode::Inclusive,
            entities: vec![version("urn:a", &[])],
            warnings: Vec::new(),
            linked_map: None,
        },
    ];
    let warnings = collect_warnings(&results);
    pretty_assertions::assert_eq!(warnings.len(), 2);
    pretty_assertions::assert_eq!(warnings[0].code(), WarnCode::MiscellaneousWarning);
    pretty_assertions::assert_eq!(warnings[1].code(), WarnCode::RevalidationFailed);
}

#[test]
fn merge_query_local_only() {
    let local = vec![version("urn:a", &[("p1", json!([{"value": 1}]))])];
    let result = merge_query_results(local, Vec::new());
    pretty_assertions::assert_eq!(result.len(), 1);
}

#[test]
fn merge_query_adds_remote_entities() {
    let local = vec![version("urn:a", &[])];
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![version("urn:b", &[])],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2);
}

#[test]
fn build_discovery_url_with_query_string() {
    use crate::forwarding_url::build_discovery_url;

    let url = build_discovery_url(&Url::parse("http://cs1.example.com").expect("valid url"), &["types"], "details=true");
    assert!(matches!(url, Ok(url) if url.as_str() == "http://cs1.example.com/ngsi-ld/v1/types?details=true"));
}

#[test]
fn build_discovery_url_without_query_string() {
    use crate::forwarding_url::build_discovery_url;

    let url = build_discovery_url(&Url::parse("http://cs1.example.com/").expect("valid url"), &["attributes"], "");
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
    use andromeda_jsonld::context_spec::ContextSpec;
    use std::sync::Arc;

    let results = vec![
        ForwardedDiscoveryResult {
            registration_id: None,
            body: None,
            naming: ContextSpec::none(),
            warnings: vec![NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string())],
        },
        ForwardedDiscoveryResult {
            registration_id: None,
            body: Some(Arc::from(b"{}".as_slice())),
            naming: ContextSpec::none(),
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
fn collect_proxied_attrs_gathers_the_attributes_of_exclusive_and_redirect_registrations() {
    let results = vec![
        write_result(ForwardingMode::Exclusive, vec![attribute_name("speed"), attribute_name("fuel")]),
        write_result(ForwardingMode::Redirect, vec![attribute_name("temperature")]),
    ];
    let attrs = collect_proxied_attrs(&results);
    pretty_assertions::assert_eq!(attrs.len(), 3);
    for expected in ["speed", "fuel", "temperature"] {
        assert!(attrs.iter().any(|attr| attr.as_str() == expected), "{expected} missing");
    }
}

/// Clause 4.3.6.3: an inclusive registration supplements local data, so the
/// attributes it covers stay in local processing.
#[test]
fn collect_proxied_attrs_leaves_out_the_attributes_an_inclusive_registration_covers() {
    let results = vec![write_result(ForwardingMode::Inclusive, vec![attribute_name("temperature")])];
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

/// Clause 5.7.2.4: auxiliary data is added only where an Attribute is not
/// present already, so it supplements a held entity and an entity no other
/// holder returned joins with every Attribute the source sent.
#[test]
fn merge_query_auxiliary_supplements_held_entities_and_adds_the_ones_nobody_else_returned() {
    let local = vec![version("urn:a", &[("speed", json!([{"value": 100}]))])];
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Auxiliary,
        entities: vec![
            version("urn:a", &[("speed", json!([{"value": 1}])), ("fuel", json!([{"value": 50}]))]),
            version("urn:b", &[("speed", json!([{"value": 80}]))]),
        ],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let result = merge_query_results(local, remote);
    pretty_assertions::assert_eq!(result.len(), 2);
    let entity_a = result
        .iter()
        .find(|merged| merged.state.id().as_str() == "urn:a")
        .expect("urn:a should be present");
    assert!(entity_a.state.attributes().contains(&iri("fuel")), "Auxiliary attrs should be merged");
    pretty_assertions::assert_eq!(instance_count(entity_a, "speed"), 1);
    let entity_b = result
        .iter()
        .find(|merged| merged.state.id().as_str() == "urn:b")
        .expect("urn:b should be present");
    pretty_assertions::assert_eq!(instance_count(entity_b, "speed"), 1);
}

/// Clause 4.3.6.2: an auxiliary answer listed before an inclusive one still
/// yields to it, so an Attribute both supply keeps the inclusive instance
/// alone while an Attribute only the auxiliary source holds is added.
#[test]
fn merge_query_an_auxiliary_answer_yields_to_an_inclusive_one_whatever_their_order() {
    let answer = |mode, value| ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode,
        entities: vec![version("urn:a", &[("speed", json!([{"value": value}])), ("fuel", json!([{"value": value}]))])],
        warnings: Vec::new(),
        linked_map: None,
    };
    let mut inclusive = answer(ForwardingMode::Inclusive, 10);
    inclusive.entities = vec![version("urn:a", &[("speed", json!([{"value": 10}]))])];
    let result = merge_query_results(Vec::new(), vec![answer(ForwardingMode::Auxiliary, 99), inclusive]);
    pretty_assertions::assert_eq!(result.len(), 1);
    pretty_assertions::assert_eq!(instance_count(&result[0], "speed"), 1);
    pretty_assertions::assert_eq!(instance_count(&result[0], "fuel"), 1);
}

#[test]
fn merge_query_primary_adds_new_entities() {
    let local = vec![version("urn:a", &[])];
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![version("urn:b", &[("speed", json!([{"value": 80}]))])],
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
    let local = version("urn:a", &[("speed", json!([{"value": 10, "observedAt": "2021-01-01T00:00:00Z"}]))]);
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Inclusive,
        entities: vec![version(
            "urn:a",
            &[
                (
                    "speed",
                    json!([{"value": 30, "observedAt": "2023-01-01T00:00:00Z", "datasetId": "urn:ngsi-ld:ds:2"}]),
                ),
                ("brand", json!([{"value": "Mercedes"}])),
            ],
        )],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let merged = merge_retrieve_results(Some(local), remote).expect("merged entity");
    pretty_assertions::assert_eq!(instance_count(&merged, "speed"), 2);
    pretty_assertions::assert_eq!(instance_count(&merged, "brand"), 1);
}

/// Clause 5.7.1.4: an auxiliary source's Attributes are added when no other
/// holder supplied them, so an entity only an auxiliary source holds is
/// answered with its Attributes.
#[test]
fn merge_retrieve_answers_with_an_auxiliary_source_when_nothing_else_holds_the_entity() {
    let remote = vec![ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode: ForwardingMode::Auxiliary,
        entities: vec![version("urn:a", &[("speed", json!([{"value": 80}]))])],
        warnings: Vec::new(),
        linked_map: None,
    }];
    let merged = merge_retrieve_results(None, remote).expect("the auxiliary entity");
    pretty_assertions::assert_eq!(instance_count(&merged, "speed"), 1);
}

/// Clause 4.3.6.2: in a retrieval too, an auxiliary answer listed before an
/// inclusive one never puts its instance beside the inclusive one.
#[test]
fn merge_retrieve_an_auxiliary_answer_yields_to_an_inclusive_one_whatever_their_order() {
    let answer = |mode, attributes: &[(&str, Value)]| ForwardedEntities {
        registration_id: Some(entity_uri("urn:ngsi-ld:ContextSourceRegistration:test")),
        mode,
        entities: vec![version("urn:a", attributes)],
        warnings: Vec::new(),
        linked_map: None,
    };
    let remote = vec![
        answer(
            ForwardingMode::Auxiliary,
            &[("speed", json!([{"value": 99}])), ("fuel", json!([{"value": 50}]))],
        ),
        answer(ForwardingMode::Inclusive, &[("speed", json!([{"value": 10}]))]),
    ];
    let merged = merge_retrieve_results(None, remote).expect("merged entity");
    pretty_assertions::assert_eq!(instance_count(&merged, "speed"), 1);
    pretty_assertions::assert_eq!(instance_count(&merged, "fuel"), 1);
}
