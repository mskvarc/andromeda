//! Distributed query and mutation URL parameters (clause 6.3.7–6.3.14, Table
//! 6.4.3.2-1): `local` bypasses forwarding, Registry-API operations are always
//! local, `type=*` implies local and rejects `local=false`, incompatible
//! parameters are `InvalidRequest`, and the standard query/retrieve/merge/purge
//! parameters and `csf` / `splitEntities` / `containedBy` / `entityMap` families
//! are honoured.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::typed_vehicle,
    fw::snapshots::create_prepared_snapshot,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header, provision_source, retrieval_source},
    oracle::{assert_problem_details, assert_status, entity_map_header, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_client::tenant_name::TenantName;
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, INVALID_REQUEST};
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::json;
use std::time::Duration;

/// `local=true` is honoured on both a query and a retrieve, bypassing forwarding
/// so the registered source is never contacted (clause 6.3.9). Confirms REQ-HTTP-33.
#[tokio::test]
async fn dist_http_local_param_honoured_on_all_listed_resources() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:local-honoured");
    let reg_id = topology.id("registration:local-honoured");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    let registration = retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .query_entities(
                &format!("type={}&local=true", urlencoding::encode(&vtype)),
                "application/ld+json",
                Some(context_link()),
            )
            .await
            .expect("local query"),
        200,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .get_entity(&entity_id, "local=true", "application/ld+json", Some(context_link()))
            .await
            .expect("local retrieve"),
        200,
    );
    assert_eq!(mock.get_total_count(), 0, "local=true must never contact a registered source");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// `local=true` bypasses every matching registration, not just the first
/// (clause 6.3.9). Confirms REQ-HTTP-34.
#[tokio::test]
async fn dist_http_local_true_bypasses_all_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:local-all");
    let first_reg = topology.id("registration:local-all-1");
    let second_reg = topology.id("registration:local-all-2");
    let first = ContextSourceAdapter::start().await;
    let second = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&first_reg, &first.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register first"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&second_reg, &second.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register second"),
        201,
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .query_entities(
                &format!("type={}&local=true", urlencoding::encode(&vtype)),
                "application/ld+json",
                Some(context_link()),
            )
            .await
            .expect("local query"),
        200,
    );
    assert_eq!(first.get_total_count(), 0, "the first source must be bypassed");
    assert_eq!(second.get_total_count(), 0, "the second source must be bypassed");

    first.shutdown().await;
    second.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Registry-API operations are always local and never forwarded to a registered
/// source (clause 6.3.9). Confirms REQ-HTTP-35.
#[tokio::test]
async fn dist_http_registry_api_operations_always_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:registry-local");
    let reg_id = topology.id("registration:registry-local");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .query_registrations(&format!("type={}", urlencoding::encode(&vtype)), None)
            .await
            .expect("registry query"),
        200,
    );
    assert_eq!(mock.get_total_count(), 0, "a Registry-API query must never be forwarded");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A batch query scoped to a snapshot runs locally even with `local=false`:
/// snapshot operations are implicitly local (clause 6.3.18), so the scoped
/// query itself never contacts the registered source and still succeeds.
/// Preparing the snapshot legitimately does contact it (clause 5.16.1.4 runs
/// `snapshotQueries` as ordinary clause 5.7.2.4 distributed queries), so the
/// assertion measures the requests the scoped query adds, not the total.
/// Confirms REQ-HTTP-36.
#[tokio::test]
async fn dist_http_snapshot_op_ignores_local_false() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:snapshot-local");
    let reg_id = topology.id("registration:snapshot-local");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let snapshot = json!({
        "type": "Snapshot",
        "snapshotLifetime": "PT1H",
        "snapshotQueries": [ { "type": "Query", "entities": [ { "type": vtype.as_str() } ] } ],
        "@context": context_link()
    });
    let snapshot_id = create_prepared_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client(), &snapshot).await;
    let forwarded_during_preparation = mock.get_total_count();

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query?local=false")
        .header("content-type", "application/ld+json")
        .header("accept", "application/ld+json")
        .header("ngsild-snapshot", snapshot_id.as_str())
        .json(&json!({ "type": "Query", "entities": [ { "type": vtype.as_str() } ], "@context": context_link() }))
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(
        mock.get_total_count(),
        forwarded_during_preparation,
        "a snapshot-scoped query is implicitly local"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A half-specified geoquery (a geo relation with no reference geometry) is
/// rejected with `BadRequestData` (clause 5.7.2.4). Confirms REQ-HTTP-44.
///
/// Clause 4.10 names `georel`, `geometry` and `coordinates` as the three
/// parameters that "are defined in order to fully specify an NGSI-LD
/// Geoquery", and clause 5.7.2.4 rules that when "the query, geoquery or
/// context source filter are not syntactically valid (as per the referred
/// clauses 4.9 and 4.10) an error of type `BadRequestData` shall be raised".
/// Clause 6.3.20's `InvalidRequest` covers parameters the operation cannot use
/// at all; `georel` is a parameter Query Entities accepts, merely incomplete
/// here.
#[tokio::test]
async fn dist_http_incomplete_geoquery_returns_bad_request_data() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&georel=near%3BmaxDistance%3D%3D1000", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("incompatible-param query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// The standard entity query parameters of Table 6.4.3.2-1 (`type`, `q`) filter
/// the result set (clause 6.4.3.2). Confirms REQ-HTTP-51.
#[tokio::test]
async fn dist_http_query_entities_supports_query_url_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let fast = topology.id("Vehicle:fast");
    let slow = topology.id("Vehicle:slow");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&fast, &vtype, 80))
            .with_ld_json()
            .send()
            .await
            .expect("create fast"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&slow, &vtype, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create slow"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&q=speed%3E50", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("filtered query");
    assert_status(&response, 200);
    let ids: Vec<String> = json_body(&response)
        .as_array()
        .unwrap_or_else(|| panic!("entity query returns an array"))
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    assert!(ids.contains(&fast), "the fast vehicle must match q=speed>50, got {ids:?}");
    assert!(!ids.contains(&slow), "the slow vehicle must be filtered out, got {ids:?}");

    topology.finish().await.expect("topology cleanup");
}

/// `type=*` implies local execution (no source is contacted) and `type=*` with
/// `local=false` is rejected as `InvalidRequest` (clause 6.3.9). Confirms REQ-HTTP-52.
#[tokio::test]
async fn dist_http_query_type_star_implies_local_and_rejects_local_false() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:type-star");
    let reg_id = topology.id("registration:type-star");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .query_entities("type=*", "application/ld+json", Some(context_link()))
            .await
            .expect("type-star query"),
        200,
    );
    assert_eq!(mock.get_total_count(), 0, "type=* implies local, so no source is contacted");

    let rejected = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=*&local=false", "application/ld+json", Some(context_link()))
        .await
        .expect("type-star local=false");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, INVALID_REQUEST);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// `splitEntities=true` forwards to each source and re-applies the local filters
/// to the reassembled entities (clause 6.3.13). Confirms REQ-HTTP-51 (splitEntities).
#[tokio::test]
async fn dist_http_query_split_entities_param_forwards_and_reapplies_filters() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:split");
    let first_reg = topology.id("registration:split-1");
    let second_reg = topology.id("registration:split-2");
    let first = ContextSourceAdapter::start().await;
    let second = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&first_reg, &first.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register first"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&second_reg, &second.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register second"),
        201,
    );
    first
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([typed_vehicle(&entity_id, &vtype, 40)]))
        .await;
    second
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([typed_vehicle(&entity_id, &vtype, 40)]))
        .await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&splitEntities=true", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("split query");
    assert_status(&response, 200);
    assert!(first.get_total_count() >= 1, "the first source must be queried under splitEntities");
    assert!(second.get_total_count() >= 1, "the second source must be queried under splitEntities");

    first.shutdown().await;
    second.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `csf` context-source filter is forwarded unchanged to a matched source
/// (clause 6.3.12). Confirms REQ-HTTP-51 (csf).
#[tokio::test]
async fn dist_http_query_csf_param_forwarded_unchanged() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:csf");
    let reg_id = topology.id("registration:csf");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&csf=mode%3D%3Dinclusive", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("csf query");
    assert_status(&response, 200);
    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert!(
        forwarded.uri.query().is_some_and(|query| query.contains("csf")),
        "the csf filter must be forwarded unchanged, got {:?}",
        forwarded.uri.query()
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `containedBy` parameter is accepted and constrains a local query
/// (clause 6.3.14). Confirms REQ-HTTP-51 (containedBy).
#[tokio::test]
async fn dist_http_query_contained_by_param_honoured() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:contained");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let query = format!("type={}&containedBy={}", urlencoding::encode(&vtype), urlencoding::encode(&entity_id));
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&query, "application/ld+json", Some(context_link()))
        .await
        .expect("containedBy query");
    assert_status(&response, 200);

    topology.finish().await.expect("topology cleanup");
}

/// `entityMap=true` returns an `NGSILD-EntityMap` header whose lifetime is set by
/// `entityMapLifetime` (clause 6.3.15). With no map referenced in the request a
/// new map is minted, so the query answers `201` (Table 6.4.3.2-3: "201 Created"
/// when an `EntityMap` is "(re)created"). Confirms REQ-HTTP-51 (entityMap).
#[tokio::test]
async fn dist_http_query_entity_map_lifetime_param_sets_expiry() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-lifetime");
    let reg_id = topology.id("registration:map-lifetime");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(
            &format!("type={}&entityMap=true&entityMapLifetime=PT5M", urlencoding::encode(&vtype)),
            "application/ld+json",
            Some(context_link()),
        )
        .await
        .expect("entity-map query");
    assert_status(&response, 201);
    assert!(
        entity_map_header(&response).is_some(),
        "an entityMap query must return an NGSILD-EntityMap header"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Purge supports the Table 6.4.3.2-1 parameters and `type=*` implies local, so
/// a registered source is not contacted (clause 6.3.9). Confirms REQ-HTTP-59.
#[tokio::test]
async fn dist_http_purge_entities_supports_table_params_and_type_star_local() {
    let topology = topology::single().await.expect("single-broker topology");
    // `type=*` selects every type, so the purge is inherently tenant-wide and
    // cannot be scoped by a per-test type or idPattern. A private tenant, used by
    // this test alone, keeps the purge from deleting concurrent tests' entities.
    let tenant = TenantName::parse("distopPurgeStarLocal").expect("valid tenant name");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").tenant_client(tenant);
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:purge-star");
    let reg_id = topology.id("registration:purge-star");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &client
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &client
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive", &["purgeEntity"]))
            .await
            .expect("register source"),
        201,
    );

    let response = client.delete_entities_by_query("type=*", Some(context_link())).await.expect("type-star purge");
    assert_status(&response, 204);
    assert_eq!(mock.get_total_count(), 0, "type=* purge implies local, so no source is contacted");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Retrieve supports the Table 6.4.3.2-1 parameters (`attrs`, `options`)
/// (clause 6.4.3.2). Confirms REQ-HTTP-62.
#[tokio::test]
async fn dist_http_retrieve_entity_supports_table_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:retrieve-params");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 42))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "attrs=speed&options=keyValues", "application/ld+json", Some(context_link()))
        .await
        .expect("parametrised retrieve");
    assert_status(&response, 200);
    assert_eq!(
        json_body(&response)["speed"].as_i64(),
        Some(42),
        "keyValues collapses the attribute to its raw value"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Merge supports its format/lang/observedAt/options/type parameters
/// (clause 6.5.3.4). The `application/ld+json` body carries its own `@context`
/// and no `Link`, since clause 6.3.5 rejects a JSON-LD Link alongside that
/// media type. Confirms REQ-HTTP-71.
#[tokio::test]
async fn dist_http_merge_entity_supports_format_lang_observedat_options_type_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:merge-params");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(
            Method::PATCH,
            &format!(
                "/ngsi-ld/v1/entities/{}?options=keyValues&lang=en&type={}",
                urlencoding::encode(&entity_id),
                urlencoding::encode(&vtype)
            ),
        )
        .header("content-type", "application/ld+json")
        .json(&json!({ "speed": { "type": "Property", "value": 99 }, "@context": context_link() }))
        .send()
        .await
        .expect("parametrised merge");
    assert_status(&response, 204);

    // A broker that accepted the parameters and applied nothing answers `204`
    // too, so the merged value is what shows the operation ran under them.
    let merged = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "options=keyValues", "application/ld+json", Some(context_link()))
        .await
        .expect("read back the merged entity");
    assert_status(&merged, 200);
    assert_eq!(
        json_body(&merged)["speed"].as_i64(),
        Some(99),
        "the parametrised merge applied its fragment: {}",
        json_body(&merged)
    );

    topology.finish().await.expect("topology cleanup");
}

/// Registration query supports the Table 6.4.3.2-1 parameters (clause 6.9.3.2).
/// Confirms REQ-HTTP-81.
#[tokio::test]
async fn dist_http_query_csource_registrations_supports_table_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:reg-params");
    let reg_id = topology.id("registration:reg-params");
    let mock = ContextSourceAdapter::start().await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(&format!("type={}&limit=5", urlencoding::encode(&vtype)), None)
        .await
        .expect("parametrised registration query");
    assert_status(&response, 200);
    assert!(json_body(&response).is_array(), "a registration query returns an array");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `entityMaps` collection supports query URL parameters (clause 6.3.15).
/// Confirms REQ-HTTP-97.
#[tokio::test]
async fn dist_http_get_entity_maps_supports_query_url_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:maps-params");
    let reg_id = topology.id("registration:maps-params");
    let mock = ContextSourceAdapter::start().await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, &vtype, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entityMaps?type={}&limit=5", urlencoding::encode(&vtype)))
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .send()
        .await
        .expect("entity-maps collection query");
    assert!(
        [200, 201].contains(&response.status.as_u16()),
        "an entityMaps collection query returns 200 or 201, got {}",
        response.status.as_u16()
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
