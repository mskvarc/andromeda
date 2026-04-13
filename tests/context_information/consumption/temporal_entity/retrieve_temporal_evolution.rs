#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    TestCtx,
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode, header::HeaderMap};

const PREFIX: &str = "consumption::temporal::retrieve";

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

/// Like `normalize` but also strips `deletedAt` keys.
fn normalize_strip_deleted_at(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            map.remove("instanceId");
            map.remove("@context");
            map.remove("deletedAt");
            for v in map.values_mut() {
                normalize_strip_deleted_at(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_strip_deleted_at(v);
            }
            arr.sort_by_key(|a| a.to_string());
        }
        _ => {}
    }
}

/// Parse Content-Range header: `date-time <start>-<end>/<size>`
fn parse_content_range(headers: &HeaderMap) -> (String, String, String, String) {
    let val = headers.get("content-range").expect("missing Content-Range header").to_str().unwrap();
    let re = regex::Regex::new(r"([a-zA-Z\-]+) (.*-.*-.*)-(.*-.*-.*)/(.*)").unwrap();
    let caps = re.captures(val).expect("Content-Range format mismatch");
    (caps[1].to_string(), caps[2].to_string(), caps[3].to_string(), caps[4].to_string())
}

fn collect_timestamps_normalized(body: &serde_json::Value) -> Vec<String> {
    let mut timestamps = Vec::new();
    for attr in ["speed", "fuelLevel"] {
        if let Some(arr) = body.get(attr).and_then(|v| v.as_array()) {
            for instance in arr {
                if let Some(ts) = instance.get("observedAt").and_then(|v| v.as_str()) {
                    timestamps.push(ts.to_string());
                }
            }
        }
    }
    timestamps
}

fn collect_timestamps_temporal_values(body: &serde_json::Value) -> Vec<String> {
    let mut timestamps = Vec::new();
    for attr in ["speed", "fuelLevel"] {
        if let Some(values) = body.get(attr).and_then(|v| v.get("values")).and_then(|v| v.as_array()) {
            for entry in values {
                if let Some(ts) = entry.get(1).and_then(|v| v.as_str()) {
                    timestamps.push(ts.to_string());
                }
            }
        }
    }
    timestamps
}

fn get_least_recent_timestamp(body: &serde_json::Value, representation: &str) -> String {
    let timestamps = if representation == "temporalValues" {
        collect_timestamps_temporal_values(body)
    } else {
        collect_timestamps_normalized(body)
    };
    timestamps.into_iter().min().expect("no timestamps found")
}

fn get_most_recent_timestamp(body: &serde_json::Value, representation: &str) -> String {
    let timestamps = if representation == "temporalValues" {
        collect_timestamps_temporal_values(body)
    } else {
        collect_timestamps_normalized(body)
    };
    timestamps.into_iter().max().expect("no timestamps found")
}

async fn create_temporal_and_track(ctx: &TestCtx, fixture: &str, id: &str) {
    ctx.track_temporal_entity(id);
    let entity = fixtures::load_fixture(fixture, id);
    ctx.client.create_temporal_entity(&entity).await;
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 020_01: Basic retrieve ────────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_01_01::basic"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_01_01:{}", uuid::Uuid::new_v4());
        // Create with original test-suite context so attributes expand to full IRIs
        ctx.track_temporal_entity(&id);
        let entity = fixtures::load_fixture_original_context("temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id);
        ctx.client.create_temporal_entity(&entity).await;

        // Retrieve without context — response has expanded IRIs
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), RequestHeaders::new()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture_original_context("temporal_entities/expectations/vehicle-temporal-representation-020-01.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_02: Retrieve with context ─────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_02_01::with_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_02_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-02.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_03: Retrieve certain attributes ───────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_03_01::attrs_fuel_level"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_03_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().attrs(["fuelLevel"]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-03-01.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_03_02::attrs_fuel_level_and_speed"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_03_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().attrs(["fuelLevel", "speed"]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-03-02.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_04: Retrieve with temporal query ──────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_04_01::timerel_after"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_04_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().timerel("after").timeat("2020-08-01T13:03:00Z");
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-04-01.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_04_02::timerel_before"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_04_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().timerel("before").timeat("2020-08-01T12:05:00Z");
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-04-02.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_04_03::timerel_between"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_04_03:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new()
            .timerel("between")
            .timeat("2020-08-01T12:00:00Z")
            .endtimeat("2020-08-01T13:00:00Z");
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-04-03.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_05: Retrieve lastN instances ──────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_05_01::last_n_4"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_05_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation-multiple-instances.jsonld", &id).await;

        let params = QueryParams::new().lastn(4);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-05-01.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_05_02::last_n_7"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_05_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation-multiple-instances.jsonld", &id).await;

        let params = QueryParams::new().lastn(7);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-05-02.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_06: Invalid entity ID ─────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_06_01::invalid_id"), |ctx| async move {
        let resp = ctx.client.get_temporal_entity("invalidUri", &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
        assert!(body["title"].as_str().is_some());
    }));

    // ── 020_07: Non-existing entity ───────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_07_01::not_found"), |ctx| async move {
        let resp = ctx
            .client
            .get_temporal_entity("urn:ngsi-ld:Vehicle:unknowEntity", &QueryParams::new(), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap_or("").contains("ResourceNotFound"));
        assert!(body["title"].as_str().is_some());
    }));

    // ── 020_08: Non-existing attributes ───────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_08_01::unknown_attrs"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_08_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().attrs(["unknownAttribute"]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap_or("").contains("ResourceNotFound"));
    }));

    // ── 020_09: Invalid temporal request ──────────────────────────────────

    for (suffix, label, timerel, timeat, endtimeat) in [
        ("020_09_01", "after_missing_timeat", "after", None, None::<&str>),
        ("020_09_02", "before_missing_timeat", "before", None, None),
        ("020_09_03", "between_missing_endtimeat", "between", Some("2020-08-01T12:00:00Z"), None),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::{label}"), move |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:{suffix}:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

            let mut params = QueryParams::new().timerel(timerel);
            if let Some(t) = timeat {
                params = params.timeat(t);
            }
            if let Some(e) = endtimeat {
                params = params.endtimeat(e);
            }
            let resp = ctx.client.get_temporal_entity(&id, &params, Default::default()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
            let body: serde_json::Value = resp.json();
            assert!(body["type"].as_str().unwrap_or("").contains("BadRequestData"));
        }));
    }

    // ── 020_10: Simplified temporal representation ────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_10_01::temporal_values"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_10_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().options([NgsiLdOption::TemporalValues]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-020-10.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_11: Aggregated temporal representation ────────────────────────

    for (suffix, label, aggr_methods, aggr_period, attrs, expectation) in [
        ("020_11_01", "avg_pt1h", "avg", "PT1H", None, "vehicle-temporal-representation-020-11-01.json"),
        (
            "020_11_02",
            "avg_pt1h_fuel_level",
            "avg",
            "PT1H",
            Some(vec!["fuelLevel"]),
            "vehicle-temporal-representation-020-11-02.json",
        ),
        (
            "020_11_03",
            "avg_max_pt1h",
            "avg,max",
            "PT1H",
            None,
            "vehicle-temporal-representation-020-11-03.json",
        ),
        (
            "020_11_04",
            "min_max_p1d",
            "min,max",
            "P1D",
            None,
            "vehicle-temporal-representation-020-11-04.json",
        ),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::aggregated_{label}"), move |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:{suffix}:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(&ctx, "temporal_entities/2020-08-vehicle-temporal-representation.jsonld", &id).await;

            let mut params = QueryParams::new()
                .options([NgsiLdOption::AggregatedValues])
                .aggr_methods(aggr_methods)
                .aggr_period_duration(aggr_period);
            if let Some(a) = attrs {
                params = params.attrs(a);
            }
            let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_fixture(&format!("temporal_entities/expectations/{expectation}"), &id);
            normalize(&mut actual);
            normalize(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // ── 020_12: LanguageProperty representation ───────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_12_01::language_property_normalized"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_12_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-language-property-temporal-representation.jsonld", &id).await;

        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-language-property-normalized-temporal-representation.jsonld",
            &id,
        );
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_12_02::language_property_simplified"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_12_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-language-property-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().options([NgsiLdOption::TemporalValues]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-language-property-simplified-temporal-representation.jsonld",
            &id,
        );
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_13: Temporal pagination ───────────────────────────────────────

    for (suffix, label, params_fn, status, check_fn) in pagination_test_cases() {
        trials.push(trial(&format!("{PREFIX}::{suffix}::pagination_{label}"), move |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:{suffix}:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(
                &ctx,
                "temporal_entities/pagination/2020-01-vehicule-temporal-representation-twenty-instances.jsonld",
                &id,
            )
            .await;

            let params = params_fn();
            let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, status);
            let body: serde_json::Value = resp.json();
            check_fn(&body, &resp.headers);
        }));
    }

    // ── 020_14: Pagination with unsynchronized attributes ─────────────────

    trials.push(trial(&format!("{PREFIX}::020_14_01::pagination_unsync_after"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_14_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(
            &ctx,
            "temporal_entities/pagination/2020-01-vehicule-temporal-representation-sixty-instances.jsonld",
            &id,
        )
        .await;

        let params = QueryParams::new().timerel("after").timeat("2019-01-01T01:01:00Z");
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        let fuel_level = body.get("fuelLevel").and_then(|v| v.as_array());
        assert!(fuel_level.is_none_or(|a| a.is_empty()), "fuelLevel should be empty");
    }));

    trials.push(trial(&format!("{PREFIX}::020_14_02::pagination_unsync_before"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_14_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(
            &ctx,
            "temporal_entities/pagination/2020-01-vehicule-temporal-representation-sixty-instances.jsonld",
            &id,
        )
        .await;

        let params = QueryParams::new().lastn(100).timerel("before").timeat("2021-01-01T01:01:00Z");
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        let speed = body.get("speed").and_then(|v| v.as_array());
        assert!(speed.is_none_or(|a| a.is_empty()), "speed should be empty");
    }));

    // ── 020_15: JsonProperty representation ───────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_15_01::json_property_normalized"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_15_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-json-property-temporal-representation.jsonld", &id).await;

        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-json-property-temporal-representation.json", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_15_02::json_property_simplified"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_15_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-json-property-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().options([NgsiLdOption::TemporalValues]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-json-property-simplified-temporal-representation.json",
            &id,
        );
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_16: VocabProperty representation ──────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_16_01::vocab_property_normalized"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_16_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-vocab-property-temporal-representation.jsonld", &id).await;

        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-vocab-property-temporal-representation.json", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    trials.push(trial(&format!("{PREFIX}::020_16_02::vocab_property_simplified"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_16_02:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-vocab-property-temporal-representation.jsonld", &id).await;

        let params = QueryParams::new().options([NgsiLdOption::TemporalValues]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-vocab-property-simplified-temporal-representation.json",
            &id,
        );
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_17: DeletedAt temporal property ───────────────────────────────

    for (suffix, attr_name, expectation) in [
        ("020_17_01", "fuelLevel", "vehicle-temporal-representation-property-020-17.jsonld"),
        ("020_17_02", "isParkedIn", "vehicle-temporal-representation-relationship-020-17.jsonld"),
        ("020_17_03", "name", "vehicle-temporal-representation-languageproperty-020-17.jsonld"),
    ] {
        trials.push(trial(&format!("{PREFIX}::{suffix}::deleted_at_{attr_name}"), move |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:{suffix}:{}", uuid::Uuid::new_v4());
            create_temporal_and_track(&ctx, "temporal_entities/vehicle-temporal-representation-different-attributes-types.jsonld", &id).await;

            ctx.client
                .request(Method::DELETE, &format!("/entities/{id}/attrs/{attr_name}"), None, ctx_headers())
                .await;

            let params = QueryParams::new().timeproperty("deletedAt");
            let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let mut actual: serde_json::Value = resp.json();
            assert!(actual[attr_name][0]["deletedAt"].is_string(), "deletedAt should be present on {attr_name}[0]");

            let mut expected = fixtures::load_fixture(&format!("temporal_entities/expectations/{expectation}"), &id);
            normalize_strip_deleted_at(&mut actual);
            normalize_strip_deleted_at(&mut expected);
            pretty_assertions::assert_eq!(expected, actual);
        }));
    }

    // ── 020_18: DeletedAt with temporal values ────────────────────────────

    for (suffix, attr_name, simplified_key, expectation) in [
        (
            "020_18_01",
            "fuelLevel",
            "values",
            "vehicle-temporal-values-representation-property-020-18.jsonld",
        ),
        (
            "020_18_02",
            "isParkedIn",
            "objects",
            "vehicle-temporal-values-representation-relationship-020-18.jsonld",
        ),
        (
            "020_18_03",
            "name",
            "languageMaps",
            "vehicle-temporal-values-representation-languageproperty-020-18.jsonld",
        ),
    ] {
        trials.push(trial(
            &format!("{PREFIX}::{suffix}::deleted_at_temporal_values_{attr_name}"),
            move |ctx| async move {
                let id = format!("urn:ngsi-ld:Vehicle:{suffix}:{}", uuid::Uuid::new_v4());
                create_temporal_and_track(&ctx, "temporal_entities/vehicle-temporal-representation-different-attributes-types.jsonld", &id).await;

                ctx.client
                    .request(Method::DELETE, &format!("/entities/{id}/attrs/{attr_name}"), None, ctx_headers())
                    .await;

                let params = QueryParams::new().timeproperty("deletedAt").options([NgsiLdOption::TemporalValues]);
                let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
                pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

                let mut actual: serde_json::Value = resp.json();
                assert!(actual[attr_name][simplified_key][0][1].is_string(), "deletedAt timestamp should be present");

                let mut expected = fixtures::load_fixture(&format!("temporal_entities/expectations/{expectation}"), &id);
                actual[attr_name][simplified_key][0][1] = serde_json::Value::Null;
                if expected[attr_name][simplified_key][0][1].is_string() {
                    expected[attr_name][simplified_key][0][1] = serde_json::Value::Null;
                }
                normalize(&mut actual);
                normalize(&mut expected);
                pretty_assertions::assert_eq!(expected, actual);
            },
        ));
    }

    // ── 020_19: DeletedAt on scope ────────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::020_19_01::deleted_at_scope"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_19_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-temporal-representation-scope.jsonld", &id).await;

        ctx.client
            .request(Method::DELETE, &format!("/entities/{id}/attrs/scope"), None, ctx_headers())
            .await;

        let params = QueryParams::new().timeproperty("deletedAt");
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        assert!(actual["scope"][0]["deletedAt"].is_string(), "deletedAt should be present on scope[0]");

        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-property-020-19.jsonld", &id);
        normalize_strip_deleted_at(&mut actual);
        normalize_strip_deleted_at(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 020_20: DeletedAt with temporal values on scope ───────────────────

    trials.push(trial(&format!("{PREFIX}::020_20_01::deleted_at_temporal_values_scope"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:020_20_01:{}", uuid::Uuid::new_v4());
        create_temporal_and_track(&ctx, "temporal_entities/vehicle-temporal-representation-scope.jsonld", &id).await;

        ctx.client
            .request(Method::DELETE, &format!("/entities/{id}/attrs/scope"), None, ctx_headers())
            .await;

        let params = QueryParams::new().timeproperty("deletedAt").options([NgsiLdOption::TemporalValues]);
        let resp = ctx.client.get_temporal_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        assert!(
            actual["scope"]["values"][0][1].is_string(),
            "deletedAt timestamp should be present at scope.values[0][1]"
        );

        actual["scope"]["values"][0][1] = serde_json::Value::Null;
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-property-020-20.jsonld", &id);
        if expected["scope"]["values"][0][1].is_string() {
            expected["scope"]["values"][0][1] = serde_json::Value::Null;
        }
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));
}

type CheckFn = Box<dyn Fn(&serde_json::Value, &HeaderMap) + Send + Sync>;
type ParamsFn = Box<dyn Fn() -> QueryParams + Send + Sync>;

fn pagination_test_cases() -> Vec<(&'static str, &'static str, ParamsFn, StatusCode, CheckFn)> {
    vec![
        (
            "020_13_01",
            "basic",
            Box::new(QueryParams::new),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "*");
                let lrt = get_least_recent_timestamp(body, "");
                let mrt = get_most_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, lrt);
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "020_13_02",
            "last_n",
            Box::new(|| QueryParams::new().lastn(20)),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "20");
                let lrt = get_least_recent_timestamp(body, "");
                let mrt = get_most_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, mrt);
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
        (
            "020_13_03",
            "last_n_before",
            Box::new(|| QueryParams::new().lastn(20).timerel("before").timeat("2021-01-01T01:01:00Z")),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, _range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "20");
                let lrt = get_least_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, "2021-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(_range_end, lrt);
            }),
        ),
        (
            "020_13_04",
            "last_n_between",
            Box::new(|| {
                QueryParams::new()
                    .lastn(20)
                    .timerel("between")
                    .timeat("2019-01-01T01:01:00Z")
                    .endtimeat("2021-01-01T01:01:00Z")
            }),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let lrt = get_least_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, "2021-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
        (
            "020_13_05",
            "last_n_after",
            Box::new(|| QueryParams::new().lastn(20).timerel("after").timeat("2019-01-01T01:01:00Z")),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "20");
                let lrt = get_least_recent_timestamp(body, "");
                let mrt = get_most_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, mrt);
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
        (
            "020_13_06",
            "before",
            Box::new(|| QueryParams::new().timerel("before").timeat("2021-01-01T01:01:00Z")),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, _range_start, range_end, size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                pretty_assertions::assert_eq!(size, "*");
                let mrt = get_most_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "020_13_07",
            "between",
            Box::new(|| {
                QueryParams::new()
                    .timerel("between")
                    .timeat("2019-01-01T01:01:00Z")
                    .endtimeat("2021-01-01T01:01:00Z")
            }),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let mrt = get_most_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, "2019-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "020_13_08",
            "after",
            Box::new(|| QueryParams::new().timerel("after").timeat("2019-01-01T01:01:00Z")),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let mrt = get_most_recent_timestamp(body, "");
                pretty_assertions::assert_eq!(range_start, "2019-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "020_13_09",
            "temporal_values_after",
            Box::new(|| {
                QueryParams::new()
                    .options([NgsiLdOption::TemporalValues])
                    .timerel("after")
                    .timeat("2019-01-01T01:01:00Z")
            }),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let mrt = get_most_recent_timestamp(body, "temporalValues");
                pretty_assertions::assert_eq!(range_start, "2019-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, mrt);
            }),
        ),
        (
            "020_13_10",
            "temporal_values_last_n_between",
            Box::new(|| {
                QueryParams::new()
                    .options([NgsiLdOption::TemporalValues])
                    .lastn(20)
                    .timerel("between")
                    .timeat("2019-01-01T01:01:00Z")
                    .endtimeat("2021-01-01T01:01:00Z")
            }),
            StatusCode::PARTIAL_CONTENT,
            Box::new(|body, headers| {
                let (unit, range_start, range_end, _size) = parse_content_range(headers);
                pretty_assertions::assert_eq!(unit, "date-time");
                let lrt = get_least_recent_timestamp(body, "temporalValues");
                pretty_assertions::assert_eq!(range_start, "2021-01-01T01:01:00Z");
                pretty_assertions::assert_eq!(range_end, lrt);
            }),
        ),
    ]
}
