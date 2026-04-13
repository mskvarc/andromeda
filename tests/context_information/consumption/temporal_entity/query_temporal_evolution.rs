#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    TestCtx,
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode, header::HeaderMap};

const PREFIX: &str = "consumption::temporal::query";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

/// Recursively strip `instanceId` and `@context`, then sort arrays for order-independent comparison.
fn normalize(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            map.remove("instanceId");
            map.remove("@context");
            for v in map.values_mut() {
                normalize(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize(v);
            }
            arr.sort_by_key(|a| a.to_string());
        }
        _ => {}
    }
}

/// Normalize each element in a list, then sort the list by entity `id`.
fn normalize_list(val: &mut serde_json::Value) {
    if let Some(arr) = val.as_array_mut() {
        for v in arr.iter_mut() {
            normalize(v);
        }
        arr.sort_by(|a, b| {
            let a_id = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let b_id = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
            a_id.cmp(b_id)
        });
    }
}

/// Parse Content-Range header: `date-time <start>-<end>/<size>`
fn parse_content_range(headers: &HeaderMap) -> (String, String, String, String) {
    let val = headers.get("content-range").expect("missing Content-Range header").to_str().unwrap();
    let re = regex::Regex::new(r"([a-zA-Z\-]+) (.*-.*-.*)-(.*-.*-.*)/(.*)").unwrap();
    let caps = re.captures(val).expect("Content-Range format mismatch");
    (caps[1].to_string(), caps[2].to_string(), caps[3].to_string(), caps[4].to_string())
}

fn collect_timestamps_vehicles(body: &[serde_json::Value], representation: &str) -> Vec<String> {
    let mut timestamps = Vec::new();
    for entity in body {
        for attr in ["speed", "fuelLevel"] {
            if representation == "temporalValues" {
                if let Some(values) = entity.get(attr).and_then(|v| v.get("values")).and_then(|v| v.as_array()) {
                    for entry in values {
                        if let Some(ts) = entry.get(1).and_then(|v| v.as_str()) {
                            timestamps.push(ts.to_string());
                        }
                    }
                }
            } else if let Some(arr) = entity.get(attr).and_then(|v| v.as_array()) {
                for instance in arr {
                    if let Some(ts) = instance.get("observedAt").and_then(|v| v.as_str()) {
                        timestamps.push(ts.to_string());
                    }
                }
            }
        }
    }
    timestamps
}

fn get_least_recent_timestamp_vehicles(body: &[serde_json::Value], representation: &str) -> String {
    collect_timestamps_vehicles(body, representation)
        .into_iter()
        .min()
        .expect("no timestamps found")
}

fn get_most_recent_timestamp_vehicles(body: &[serde_json::Value], representation: &str) -> String {
    collect_timestamps_vehicles(body, representation)
        .into_iter()
        .max()
        .expect("no timestamps found")
}

/// Replace vehicle IDs in expectation arrays.
fn replace_vehicle_ids(expected: &mut serde_json::Value, va: &str, vb: &str) {
    if let Some(arr) = expected.as_array_mut() {
        for item in arr.iter_mut() {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()).map(String::from) {
                if id.contains("-A") {
                    item["id"] = serde_json::Value::String(va.to_string());
                } else if id.contains("-B") {
                    item["id"] = serde_json::Value::String(vb.to_string());
                }
            }
        }
    }
}

async fn create_temporal_and_track(ctx: &TestCtx, fixture: &str, id: &str) {
    ctx.track_temporal_entity(id);
    let entity = fixtures::load_fixture(fixture, id);
    ctx.client.create_temporal_entity(&entity).await;
}

async fn setup_two_vehicles(ctx: &TestCtx, prefix: &str) -> (String, String) {
    let va = format!("urn:ngsi-ld:Vehicle:{prefix}-A");
    let vb = format!("urn:ngsi-ld:Vehicle:{prefix}-B");
    create_temporal_and_track(ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
    create_temporal_and_track(ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &vb).await;
    (va, vb)
}

async fn setup_021_01(ctx: &TestCtx) -> (String, String, String) {
    let va = "urn:ngsi-ld:Vehicle:021-01-A".to_string();
    let vb = "urn:ngsi-ld:Vehicle:021-01-B".to_string();
    let bus = "urn:ngsi-ld:Bus:021-13-A".to_string();
    create_temporal_and_track(ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
    create_temporal_and_track(ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &vb).await;
    create_temporal_and_track(ctx, "temporal_entities/2020-08-bus-temporal-representation.jsonld", &bus).await;
    (va, vb, bus)
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 021_01: Query with temporal constraints ───────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_01_01::after"), |ctx| async move {
        let (va, vb, _bus) = setup_021_01(&ctx).await;

        let params = QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-08-01T12:04:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-01-01.jsonld");
        if let Some(arr) = expected.as_array_mut() {
            for item in arr.iter_mut() {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if id.contains("021-01-A") || id.contains("randomUUID") {
                        item["id"] = serde_json::Value::String(va.clone());
                    } else if id.contains("021-01-B") {
                        item["id"] = serde_json::Value::String(vb.clone());
                    }
                }
            }
        }
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::021_01_02::before"), |ctx| async move {
        let (va, vb, _bus) = setup_021_01(&ctx).await;

        let params = QueryParams::new().type_("Vehicle").timerel("before").timeat("2020-09-01T13:06:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-01-02.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::021_01_03::between"), |ctx| async move {
        let (va, vb, _bus) = setup_021_01(&ctx).await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .timerel("between")
            .timeat("2020-08-01T12:04:00Z")
            .endtimeat("2020-09-01T13:06:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-01-03.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_02: Query specific attributes ─────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_02_01::attrs_speed"), |ctx| async move {
        let (va, vb) = setup_two_vehicles(&ctx, "021-02").await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .attrs(["speed"])
            .timerel("after")
            .timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-02.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_03: Query with lastN ──────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_03_01::last_n"), |ctx| async move {
        let va = "urn:ngsi-ld:Vehicle:021-03-A".to_string();
        let vb = "urn:ngsi-ld:Vehicle:021-03-B".to_string();
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation-multiple-instances.jsonld", &va).await;
        create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &vb).await;

        let params = QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-07-01T12:05:00Z").lastn(4);
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-03.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_04: Query with context ────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_04_01::with_context"), |ctx| async move {
        let (va, vb) = setup_two_vehicles(&ctx, "021-04").await;

        let params = QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-04.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_05: Query by type ─────────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_05_01::by_type_bus"), |ctx| async move {
        let va = "urn:ngsi-ld:Vehicle:021-05-A".to_string();
        let bus = "urn:ngsi-ld:Bus:021-05-A".to_string();
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-bus-temporal-representation.jsonld", &bus).await;

        let params = QueryParams::new().type_("Bus").timerel("after").timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-05.jsonld");
        if let Some(arr) = expected.as_array_mut() {
            for item in arr.iter_mut() {
                item["id"] = serde_json::Value::String(bus.clone());
            }
        }
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_06: Query by entity ID ────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_06_01::by_id"), |ctx| async move {
        let (va, vb) = setup_two_vehicles(&ctx, "021-06").await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .id(va.clone())
            .timerel("after")
            .timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-06.jsonld");
        if let Some(arr) = expected.as_array_mut() {
            for item in arr.iter_mut() {
                item["id"] = serde_json::Value::String(va.clone());
            }
        }
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
        let _ = vb;
    }));

    // ── 021_07: Query by idPattern ────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_07_01::by_id_pattern"), |ctx| async move {
        let (va, vb) = setup_two_vehicles(&ctx, "021-07").await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .id_pattern("urn:ngsi-ld:Vehicle:.*")
            .timerel("after")
            .timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-07.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_08: Query with q filter ──────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_08_01::q_filter"), |ctx| async move {
        let (va, vb) = setup_two_vehicles(&ctx, "021-08").await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .q("speed>90")
            .timerel("after")
            .timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-08.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_09: Geo-query ─────────────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_09_01::geo_near"), |ctx| async move {
        let va = "urn:ngsi-ld:Vehicle:021-09-A".to_string();
        let vb = "urn:ngsi-ld:Vehicle:021-09-B".to_string();
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
        create_temporal_and_track(&ctx, "temporal_entities/2020-10-vehicle-temporal-representation-with-location.jsonld", &vb).await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .georel("near;maxDistance==2000")
            .geometry("Point")
            .coordinates("[-8.503,41.202]")
            .timerel("after")
            .timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-09-01.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::021_09_02::geo_within"), |ctx| async move {
        let va = "urn:ngsi-ld:Vehicle:021-09-A".to_string();
        let vb = "urn:ngsi-ld:Vehicle:021-09-B".to_string();
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
        create_temporal_and_track(&ctx, "temporal_entities/2020-10-vehicle-temporal-representation-with-location.jsonld", &vb).await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .georel("within")
            .geometry("Polygon")
            .coordinates("[[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-13.503,32.57],[-13.503,47.202]]]")
            .geoproperty("location")
            .timerel("after")
            .timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("temporal_entities/expectations/vehicles-temporal-representation-021-09-02.jsonld");
        replace_vehicle_ids(&mut expected, &va, &vb);
        normalize_list(&mut actual);
        normalize_list(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 021_11: Query with limit ──────────────────────────────────────────

    for (suffix, label, limit_val) in [("021_11_01", "limit_2", 2u32), ("021_11_02", "limit_3", 3)] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::{label}"), move |ctx| async move {
            let v1 = format!("urn:ngsi-ld:Vehicle:{suffix}a:{}", uuid::Uuid::new_v4());
            let v2 = format!("urn:ngsi-ld:Vehicle:{suffix}b:{}", uuid::Uuid::new_v4());
            let v3 = format!("urn:ngsi-ld:Vehicle:{suffix}c:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &v1).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &v2).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &v3).await;

            let params = QueryParams::new()
                .type_("Bus,Vehicle")
                .limit(limit_val)
                .timerel("after")
                .timeat("2020-07-01T12:05:00Z");
            let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(body.len() <= limit_val as usize);
        }));
    }

    // ── 021_12: Invalid request ───────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::021_12_01::invalid_no_type"), |ctx| async move {
        let params = QueryParams::new().timerel("after").timeat("2020-07-01T12:05:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
        assert!(body["title"].as_str().is_some());
    }));

    // ── 021_13: Query via POST entityOperations ───────────────────────────

    for (suffix, label, query_fixture, expectation_fixture, include_bus) in [
        (
            "021_13_01",
            "post_after",
            "entity-operations-after-query.jsonld",
            "vehicles-temporal-representation-021-13-01.jsonld",
            false,
        ),
        (
            "021_13_02",
            "post_before",
            "entity-operations-before-query.jsonld",
            "vehicles-temporal-representation-021-13-02.jsonld",
            false,
        ),
        (
            "021_13_03",
            "post_between",
            "entity-operations-between-query.jsonld",
            "vehicles-temporal-representation-021-13-03.jsonld",
            false,
        ),
        (
            "021_13_04",
            "post_after_two_entities",
            "entity-operations-after-query-two-entities.jsonld",
            "vehicles-bus-temporal-representation-021-13-04.jsonld",
            true,
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::{label}"), move |ctx| async move {
            let va = "urn:ngsi-ld:Vehicle:021-13-A".to_string();
            let vb = "urn:ngsi-ld:Vehicle:021-13-B".to_string();
            let bus = "urn:ngsi-ld:Bus:021-13-A".to_string();
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &vb).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-bus-temporal-representation.jsonld", &bus).await;

            let query = fixtures::load_fragment(&format!("temporal_entities/{query_fixture}"));
            let headers = RequestHeaders::new()
                .content_type("application/json")
                .link_context(fixtures::test_suite_context_url());
            let resp = ctx
                .client
                .request(Method::POST, "/temporal/entityOperations/query", Some(&query), headers)
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation(&format!("temporal_entities/expectations/{expectation_fixture}"));
            if let Some(arr) = expected.as_array_mut() {
                for item in arr.iter_mut() {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()).map(String::from) {
                        if id.contains("-A") && id.contains("Vehicle") {
                            item["id"] = serde_json::Value::String(va.clone());
                        } else if id.contains("-B") {
                            item["id"] = serde_json::Value::String(vb.clone());
                        } else if id.contains("Bus") && include_bus {
                            item["id"] = serde_json::Value::String(bus.clone());
                        }
                    }
                }
            }
            normalize_list(&mut actual);
            normalize_list(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // ── 021_14: Simplified temporal representation ────────────────────────

    for (suffix, label, timerel, timeat, endtimeat, expectation) in [
        (
            "021_14_01",
            "after",
            "after",
            "2020-08-01T12:04:00Z",
            None,
            "vehicles-temporal-representation-021-14-01.json",
        ),
        (
            "021_14_02",
            "before",
            "before",
            "2020-09-01T13:06:00Z",
            None,
            "vehicles-temporal-representation-021-14-02.json",
        ),
        (
            "021_14_03",
            "between",
            "between",
            "2020-08-01T12:04:00Z",
            Some("2020-09-01T13:06:00Z"),
            "vehicles-temporal-representation-021-14-03.json",
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::simplified_{label}"), move |ctx| async move {
            let va = "urn:ngsi-ld:Vehicle:021-14-A".to_string();
            let vb = "urn:ngsi-ld:Vehicle:021-14-B".to_string();
            let bus = "urn:ngsi-ld:Bus:021-14-A".to_string();
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &vb).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-bus-temporal-representation.jsonld", &bus).await;

            let mut params = QueryParams::new()
                .type_("Vehicle")
                .options([NgsiLdOption::TemporalValues])
                .timerel(timerel)
                .timeat(timeat);
            if let Some(e) = endtimeat {
                params = params.endtimeat(e);
            }
            let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation(&format!("temporal_entities/expectations/{expectation}"));
            replace_vehicle_ids(&mut expected, &va, &vb);
            normalize_list(&mut actual);
            normalize_list(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // ── 021_15-16: Query temporal pagination ─────────────────────────────

    // Pagination tests use shared setup
    for (suffix, label, params_fn, check_fn) in query_pagination_test_cases() {
        trials.push(trial(&format!("{PREFIX}::{suffix}::pagination_{label}"), move |ctx| async move {
            let v1 = format!("urn:ngsi-ld:Vehicle:021_15_a:{}", uuid::Uuid::new_v4());
            let v2 = format!("urn:ngsi-ld:Vehicle:021_15_b:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(
                &ctx,
                "temporal_entities/pagination/2020-01-vehicule-temporal-representation-twenty-instances.jsonld",
                &v1,
            )
            .await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &v2).await;

            let params = params_fn();
            let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
            let body: Vec<serde_json::Value> = resp.json();
            check_fn(&body, &resp.headers);
        }));
    }

    // 021_16_01: POST query pagination
    trials.push(trial(&format!("{PREFIX}::021_16_01::post_pagination"), |ctx| async move {
        let v1 = format!("urn:ngsi-ld:Vehicle:021_15_a:{}", uuid::Uuid::new_v4());
        let v2 = format!("urn:ngsi-ld:Vehicle:021_15_b:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(
            &ctx,
            "temporal_entities/pagination/2020-01-vehicule-temporal-representation-twenty-instances.jsonld",
            &v1,
        )
        .await;
        create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &v2).await;

        let query = fixtures::load_fragment("temporal_entities/entity-operations-before-query.jsonld");
        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx
            .client
            .request(Method::POST, "/temporal/entityOperations/query", Some(&query), headers)
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::PARTIAL_CONTENT);
        let (unit, _, _, _) = parse_content_range(&resp.headers);
        pretty_assertions::assert_eq!(unit, "date-time");
    }));

    // ── 021_17: Aggregated temporal representation ────────────────────────

    for (suffix, label, aggr_methods, aggr_period, attrs, expectation) in [
        (
            "021_17_01",
            "avg_pt1h",
            "avg",
            "PT1H",
            None,
            "vehicle-temporal-representation-aggregated-avg-PT1H.json",
        ),
        (
            "021_17_02",
            "avg_pt1h_fuel_level",
            "avg",
            "PT1H",
            Some(vec!["fuelLevel"]),
            "vehicle-temporal-representation-aggregated-avg-PT1H-fuelLevel.json",
        ),
        (
            "021_17_03",
            "avg_max_pt1h",
            "avg,max",
            "PT1H",
            None,
            "vehicle-temporal-representation-aggregated-avg-max-PT1H.json",
        ),
        (
            "021_17_04",
            "min_max_p1d",
            "min,max",
            "P1D",
            None,
            "vehicle-temporal-representation-aggregated-min-max-P1D.json",
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::aggregated_{label}"), move |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:{suffix}:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

            let mut params = QueryParams::new()
                .type_("Vehicle")
                .options([NgsiLdOption::AggregatedValues])
                .aggr_methods(aggr_methods)
                .aggr_period_duration(aggr_period)
                .timerel("after")
                .timeat("2020-01-01T12:03:00Z");
            if let Some(a) = attrs {
                params = params.attrs(a);
            }
            let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let body: Vec<serde_json::Value> = resp.json();
            let mut actual = body[0].clone();
            let mut expected = fixtures::load_fixture(&format!("temporal_entities/expectations/{expectation}"), &id);
            normalize(&mut actual);
            normalize(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // ── 021_18: Filter by datasetId ───────────────────────────────────────

    for (suffix, label, attrs, dataset_id, expectation) in [
        (
            "021_18_01",
            "attrs_speed",
            Some(vec!["speed"]),
            None,
            "vehicle-temporal-representation-speed-attribute.json",
        ),
        (
            "021_18_02",
            "dataset_common",
            None,
            Some("urn:ngsi-ld:Dataset:Common"),
            "vehicle-temporal-representation-common-datasetid.json",
        ),
        (
            "021_18_03",
            "two_dataset_ids",
            None,
            Some("urn:ngsi-ld:Dataset:Common,urn:ngsi-ld:Dataset:Speed"),
            "vehicle-temporal-representation-two-datasetids.json",
        ),
        (
            "021_18_04",
            "default_instance",
            None,
            Some("@none"),
            "vehicle-temporal-representation-default-instances.json",
        ),
        (
            "021_18_05",
            "speed_default_instance",
            Some(vec!["speed"]),
            Some("@none"),
            "vehicle-temporal-representation-speed-default-instance.json",
        ),
        (
            "021_18_06",
            "fuel_level_dataset",
            Some(vec!["fuelLevel"]),
            Some("urn:ngsi-ld:Dataset:fuel"),
            "vehicle-temporal-representation-fuellevel-attribute-fuel-datasetid.json",
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::dataset_{label}"), move |ctx| async move {
            let id = "urn:ngsi-ld:Vehicle:021-18-A".to_string();
            create_temporal_and_track(
                &ctx,
                "temporal_entities/vehicle-temporal-representation-with-multi-attributes-instances.jsonld",
                &id,
            )
            .await;

            let mut params = QueryParams::new()
                .type_("Vehicle")
                .options([NgsiLdOption::TemporalValues])
                .timerel("after")
                .timeat("2020-08-01T12:04:00Z");
            if let Some(a) = attrs {
                params = params.attrs(a);
            }
            if let Some(d) = dataset_id {
                params = params.dataset_id(d);
            }
            let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let body: Vec<serde_json::Value> = resp.json();
            assert!(!body.is_empty(), "expected at least one entity in response");
            let mut actual = body[0].clone();
            let expected_full = fixtures::load_expectation(&format!("temporal_entities/expectations/{expectation}"));
            let mut expected = match expected_full {
                serde_json::Value::Array(ref arr) => {
                    let mut e = arr[0].clone();
                    e["id"] = serde_json::Value::String(id.clone());
                    e
                }
                other => {
                    let mut e = other;
                    e["id"] = serde_json::Value::String(id.clone());
                    e
                }
            };
            normalize(&mut actual);
            normalize(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // 021_18_07: Filter no match
    trials.push(trial(&format!("{PREFIX}::021_18_07::dataset_no_match"), |ctx| async move {
        let id = "urn:ngsi-ld:Vehicle:021-18-A".to_string();
        create_temporal_and_track(
            &ctx,
            "temporal_entities/vehicle-temporal-representation-with-multi-attributes-instances.jsonld",
            &id,
        )
        .await;

        let params = QueryParams::new()
            .type_("Vehicle")
            .attrs(["speed"])
            .dataset_id("urn:ngsi-ld:Dataset:fuel")
            .options([NgsiLdOption::TemporalValues])
            .timerel("after")
            .timeat("2020-08-01T12:04:00Z");
        let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.is_empty(), "expected empty result set");
    }));

    // ── 021_19: Format/options params ─────────────────────────────────────

    for (suffix, label, format_val, options, expectation) in [
        (
            "021_19_01",
            "format_temporal_values",
            Some("temporalValues"),
            None,
            "vehicles-temporal-representation-021-19-01.json",
        ),
        (
            "021_19_02",
            "format_takes_precedence",
            Some("temporalValues"),
            Some(vec![NgsiLdOption::AggregatedValues]),
            "vehicles-temporal-representation-021-19-01.json",
        ),
        (
            "021_19_03",
            "options_temporal_values",
            None,
            Some(vec![NgsiLdOption::TemporalValues]),
            "vehicles-temporal-representation-021-19-01.json",
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::{label}"), move |ctx| async move {
            let va = "urn:ngsi-ld:Vehicle:021-19-A".to_string();
            let vb = "urn:ngsi-ld:Vehicle:021-19-B".to_string();
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &va).await;
            create_temporal_and_track(&ctx, "temporal_entities/2020-09-vehicle-temporal-representation.jsonld", &vb).await;

            let mut params = QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T12:03:00Z");
            if let Some(f) = format_val {
                params = params.format(f);
            }
            if let Some(o) = options {
                params = params.options(o);
            }
            // For 021_19_02: also add aggr_methods
            if suffix == "021_19_02" {
                params = params.aggr_methods("sum");
            }
            let resp = ctx.client.query_temporal_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation(&format!("temporal_entities/expectations/{expectation}"));
            replace_vehicle_ids(&mut expected, &va, &vb);
            normalize_list(&mut actual);
            normalize_list(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // ── 021_20: Faulty format/options ─────────────────────────────────────

    for (suffix, label, url) in [
        (
            "021_20_01",
            "invalid_format",
            "/temporal/entities?type=Vehicle&format=invalidFormat&timerel=after&timeAt=2020-01-01T12:03:00Z",
        ),
        (
            "021_20_02",
            "invalid_options",
            "/temporal/entities?type=Vehicle&options=invalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        ),
        (
            "021_20_03",
            "partial_invalid_options",
            "/temporal/entities?type=Vehicle&options=temporalValues%2CinvalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        ),
        (
            "021_20_04",
            "format_with_invalid_options",
            "/temporal/entities?type=Vehicle&format=temporalValues&options=aggregatedValues%2CinvalidOptions&timerel=after&timeAt=2020-01-01T12:03:00Z",
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::{label}"), move |ctx| async move {
            let resp = ctx.client.request(Method::GET, url, None, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
            let body: serde_json::Value = resp.json();
            assert!(body["type"].as_str().unwrap_or("").contains("InvalidRequest"));
        }));
    }
}

type PaginationCheckFn = Box<dyn Fn(&[serde_json::Value], &HeaderMap) + Send + Sync>;
type PaginationParamsFn = Box<dyn Fn() -> QueryParams + Send + Sync>;

fn query_pagination_test_cases() -> Vec<(&'static str, &'static str, PaginationParamsFn, PaginationCheckFn)> {
    vec![
        (
            "021_15_01",
            "last_n_before",
            Box::new(|| QueryParams::new().type_("Vehicle").lastn(20).timerel("before").timeat("2021-01-01T01:01:00Z")),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "20");
                let lrt = get_least_recent_timestamp_vehicles(body, "");
                pretty_assertions::assert_eq!(range_start, "2021-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
        (
            "021_15_02",
            "last_n_between",
            Box::new(|| {
                QueryParams::new()
                    .type_("Vehicle")
                    .lastn(20)
                    .timerel("between")
                    .timeat("2019-01-01T01:01:00Z")
                    .endtimeat("2021-01-01T01:01:00Z")
            }),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let lrt = get_least_recent_timestamp_vehicles(body, "");
                pretty_assertions::assert_eq!(range_start, "2021-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
        (
            "021_15_03",
            "last_n_after",
            Box::new(|| QueryParams::new().type_("Vehicle").lastn(20).timerel("after").timeat("2019-01-01T01:01:00Z")),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "20");
                let lrt = get_least_recent_timestamp_vehicles(body, "");
                let mrt = get_most_recent_timestamp_vehicles(body, "");
                pretty_assertions::assert_eq!(range_start, mrt);
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
        (
            "021_15_04",
            "before",
            Box::new(|| QueryParams::new().type_("Vehicle").timerel("before").timeat("2021-01-01T01:01:00Z")),
            Box::new(|body, headers| {
                let (unit, _range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "*");
                let mrt = get_most_recent_timestamp_vehicles(body, "");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "021_15_05",
            "between",
            Box::new(|| {
                QueryParams::new()
                    .type_("Vehicle")
                    .timerel("between")
                    .timeat("2019-01-01T01:01:00Z")
                    .endtimeat("2021-01-01T01:01:00Z")
            }),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let mrt = get_most_recent_timestamp_vehicles(body, "");
                pretty_assertions::assert_eq!(range_start, "2019-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "021_15_06",
            "after",
            Box::new(|| QueryParams::new().type_("Vehicle").timerel("after").timeat("2019-01-01T01:01:00Z")),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let mrt = get_most_recent_timestamp_vehicles(body, "");
                pretty_assertions::assert_eq!(range_start, "2019-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "021_15_07",
            "temporal_values_after",
            Box::new(|| {
                QueryParams::new()
                    .type_("Vehicle")
                    .options([NgsiLdOption::TemporalValues])
                    .timerel("after")
                    .timeat("2019-01-01T01:01:00Z")
            }),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let mrt = get_most_recent_timestamp_vehicles(body, "temporalValues");
                pretty_assertions::assert_eq!(range_start, "2019-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "021_15_08",
            "temporal_values_last_n_between",
            Box::new(|| {
                QueryParams::new()
                    .type_("Vehicle")
                    .options([NgsiLdOption::TemporalValues])
                    .lastn(20)
                    .timerel("between")
                    .timeat("2019-01-01T01:01:00Z")
                    .endtimeat("2021-01-01T01:01:00Z")
            }),
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let lrt = get_least_recent_timestamp_vehicles(body, "temporalValues");
                pretty_assertions::assert_eq!(range_start, "2021-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
    ]
}
