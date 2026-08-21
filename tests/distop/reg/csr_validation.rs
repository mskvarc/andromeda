//! Mandatory/format validation matrix over a valid baseline CSR (clause 5.2.9).
//!
//! Each row mutates exactly one field of the baseline so a `400 BadRequestData`
//! isolates a single constraint. All `T-LOCAL`, all `IMPL`.

use crate::{
    oracle::{assert_problem_details, assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::csr_basic,
    topology::DistributedTopology,
};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use serde_json::{Value, json};

/// Remove a top-level key from a CSR body, panicking if the shape is unexpected.
fn remove(body: &mut Value, key: &str) {
    body.as_object_mut().unwrap_or_else(|| unreachable!("CSR baseline is an object")).remove(key);
}

/// Generate one `#[tokio::test]` per validation row. Each arm builds the
/// baseline, applies a non-capturing mutation, POSTs, and asserts the request is
/// rejected with `400 BadRequestData`.
macro_rules! rejection_cases {
    ($($name:ident => $mutate:expr;)+) => {
        $(
            #[tokio::test]
            async fn $name() {
                let topology = DistributedTopology::single().await.expect("single-broker topology");
                let id = topology.id(concat!("registration:", stringify!($name)));
                let mut body = csr_basic(&id, PEER_ENDPOINT);
                let mutate: fn(&mut Value) = $mutate;
                mutate(&mut body);
                let response = topology.front().client().create_registration(&body).await.expect("create registration");
                assert_status(&response, 400);
                assert_problem_details(&response, BAD_REQUEST_DATA);
                topology.finish().await.expect("topology cleanup");
            }
        )+
    };
}

rejection_cases! {
    reg_entityinfo_missing_type_returns_400 => |body| body["information"][0]["entities"][0] = json!({ "id": "urn:ngsi-ld:Vehicle:V1" });
    reg_entityinfo_id_only_without_type_returns_400 => |body| body["information"][0]["entities"][0] = json!({ "id": "urn:ngsi-ld:Vehicle:V1" });
    reg_entityinfo_idpattern_only_without_type_returns_400 => |body| body["information"][0]["entities"][0] = json!({ "idPattern": "urn:ngsi-ld:Vehicle:.*" });
    reg_entityinfo_invalid_id_uri_returns_400 => |body| body["information"][0]["entities"][0] = json!({ "type": "Vehicle", "id": "not a uri" });
    reg_entityinfo_malformed_idpattern_returns_400 => |body| body["information"][0]["entities"][0] = json!({ "type": "Vehicle", "idPattern": "[unterminated" });
    reg_invalid_id_uri_returns_400 => |body| body["id"] = json!("foo");
    reg_type_omitted_returns_400 => |body| remove(body, "type");
    reg_type_not_contextsourceregistration_returns_400 => |body| body["type"] = json!("Foo");
    reg_endpoint_omitted_returns_400 => |body| remove(body, "endpoint");
    reg_endpoint_not_uri_returns_400 => |body| body["endpoint"] = json!("###not a uri###");
    reg_information_omitted_returns_400 => |body| remove(body, "information");
    reg_information_empty_array_returns_400 => |body| body["information"] = json!([]);
    reg_contextsourcealias_empty_returns_400 => |body| body["contextSourceAlias"] = json!("");
    reg_description_empty_returns_400 => |body| body["description"] = json!("");
    reg_datasetid_non_uri_returns_400 => |body| body["datasetId"] = json!(["not a uri"]);
    reg_expiresat_invalid_datetime_returns_400 => |body| body["expiresAt"] = json!("not-a-date");
    reg_location_malformed_geojson_returns_400 => |body| body["location"] = json!({ "type": "Point", "coordinates": "nope" });
    reg_mode_invalid_value_returns_400 => |body| body["mode"] = json!("foo");
    reg_observationspace_malformed_returns_400 => |body| body["observationSpace"] = json!({ "type": "Polygon", "coordinates": 5 });
    reg_operations_unknown_returns_400 => |body| body["operations"] = json!(["notAnOperation"]);
    reg_operationspace_malformed_returns_400 => |body| body["operationSpace"] = json!({ "type": "Point" });
    reg_refreshrate_invalid_duration_returns_400 => |body| body["refreshRate"] = json!("5 minutes");
    reg_registrationname_empty_returns_400 => |body| body["registrationName"] = json!("");
    reg_registrationinfo_empty_object_returns_400 => |body| body["information"] = json!([ {} ]);
    reg_registrationinfo_empty_entities_returns_400 => |body| body["information"] = json!([ { "entities": [] } ]);
    reg_registrationinfo_empty_propertynames_returns_400 => |body| body["information"] = json!([ { "propertyNames": [] } ]);
    reg_registrationinfo_empty_relationshipnames_returns_400 => |body| body["information"] = json!([ { "relationshipNames": [] } ]);
    reg_timeinterval_missing_startat_returns_400 => |body| body["managementInterval"] = json!({ "endAt": "2030-01-01T00:00:00Z" });
    reg_timeinterval_endat_before_startat_returns_400 => |body| body["observationInterval"] = json!({ "startAt": "2030-01-01T00:00:00Z", "endAt": "2020-01-01T00:00:00Z" });
    reg_management_unknown_member_returns_400 => |body| body["management"] = json!({ "managementInterval": { "startAt": "2020-01-01T00:00:00Z" } });
    reg_management_cacheduration_invalid_returns_400 => |body| body["management"] = json!({ "cacheDuration": "10s" });
    reg_management_cooldown_zero_returns_400 => |body| body["management"] = json!({ "cooldown": 0 });
    reg_management_cooldown_negative_returns_400 => |body| body["management"] = json!({ "cooldown": -5 });
    reg_management_timeout_zero_returns_400 => |body| body["management"] = json!({ "timeout": 0 });
    reg_management_timeout_negative_returns_400 => |body| body["management"] = json!({ "timeout": -1 });
    reg_contextsourceinfo_bad_accept_returns_400 => |body| body["contextSourceInfo"] = json!([ { "key": "accept", "value": "text/plain" } ]);
    reg_any_529_violation_returns_baddata_aggregate => |body| { body["mode"] = json!("foo"); remove(body, "endpoint"); };
}

/// `EntityInfo.type` may be an array of types; the CSR is accepted and both
/// types round-trip (clause 5.2.9, 4.5.4). Confirms REQ-REG-1.
#[tokio::test]
async fn reg_entityinfo_type_as_array_accepted() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:type-array");
    let mut body = csr_basic(&id, PEER_ENDPOINT);
    body["information"][0]["entities"][0] = json!({ "type": ["Vehicle", "Car"] });

    let created = topology.front().client().create_registration(&body).await.expect("create registration");
    assert_status(&created, 201);

    let retrieved = topology.front().client().retrieve_registration(&id, None).await.expect("retrieve registration");
    assert_status(&retrieved, 200);
    let stored = json_body(&retrieved);
    let types = stored["information"][0]["entities"][0]["type"]
        .as_array()
        .unwrap_or_else(|| unreachable!("type array round-trips"));
    let types: Vec<&str> = types.iter().filter_map(Value::as_str).collect();
    assert!(
        types.contains(&"Vehicle") && types.contains(&"Car"),
        "both entity types are preserved, got {types:?}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// When both `id` and `idPattern` are present the exact `id` wins and the
/// pattern is ignored (clause 5.2.9). Confirms REQ-REG-5.
#[tokio::test]
async fn reg_entityinfo_id_and_idpattern_prefers_id() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let exact = topology.id("Vehicle:EXACT");
    let other = topology.id("Vehicle:OTHER1");
    let id = topology.id("registration:id-wins");
    let mut body = csr_basic(&id, PEER_ENDPOINT);
    body["information"][0]["entities"][0] = json!({ "type": "Vehicle", "id": exact, "idPattern": format!("{}.*", topology.id("Vehicle:OTHER")) });

    let created = topology.front().client().create_registration(&body).await.expect("create registration");
    assert_status(&created, 201);

    let by_exact = topology
        .front()
        .client()
        .query_registrations(&format!("id={}", urlencoding::encode(&exact)), None)
        .await
        .expect("discovery by exact id");
    assert_status(&by_exact, 200);
    let by_exact = json_body(&by_exact);
    assert!(
        by_exact.as_array().is_some_and(|registrations| !registrations.is_empty()),
        "discovery by the exact id returns the registration"
    );

    let by_other = topology
        .front()
        .client()
        .query_registrations(&format!("id={}", urlencoding::encode(&other)), None)
        .await
        .expect("discovery by pattern-covered id");
    assert_status(&by_other, 200);
    let by_other = json_body(&by_other);
    assert!(
        by_other.as_array().is_none_or(|registrations| registrations.is_empty()),
        "the ignored idPattern must not match, so a pattern-covered id finds nothing"
    );

    topology.finish().await.expect("topology cleanup");
}
