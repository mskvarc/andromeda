use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial_clean_db,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "context_source::discovery::query";

fn strip_ignored_keys(val: &mut serde_json::Value, keys: &[&str]) {
    match val {
        serde_json::Value::Object(map) => {
            for key in keys {
                map.remove(*key);
            }
            for v in map.values_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        _ => {}
    }
}

fn link_context_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 037_01: Query with entity types or attribute names ────────────

    // 037_01_01: Query by type=Building → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_01_01::with_entity_types"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_01_01_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_01_01_b:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Building"), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-01.json");
        expected[0]["id"] = serde_json::Value::String(id2.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 037_01_02: Query by attrs=name → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_01_02::with_attribute_names"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_01_02_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_01_02_b:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().attrs(["name"]), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-01.json");
        expected[0]["id"] = serde_json::Value::String(id2.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_02: Query without entity types or attribute names ─────────

    // 037_02_01: No type or attrs → 400 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_02_01::without_type_or_attrs"), |ctx| async move {
        let resp = ctx.client.query_registrations(&QueryParams::new(), link_context_headers()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "missing type/attrs should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 037_03: Query with invalid query parameters ───────────────────

    // 037_03_01: Invalid id → 400 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_03_01::invalid_id"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceRegistrations?id=invalidUri", None, link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // 037_03_02: Invalid q → 400 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_03_02::invalid_query"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceRegistrations?q=(invalidQuery", None, link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // 037_03_03: Invalid georel → 400 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_03_03::invalid_geoquery"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceRegistrations?georel=within", None, link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // 037_03_04: Invalid timerel → 400 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_03_04::invalid_temporal_query"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceRegistrations?timerel=before", None, link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // ── 037_04: Query without context ─────────────────────────────────

    // 037_04_01: Query with FQ type, no Link context → 200 (5.10.2)
    // Uses original context so entity types are expanded to FQNs during creation.
    // Query uses fully qualified type URI without Link header — spec 5.10.2.3
    // says "Both type name (short hand string) and fully qualified type name (URI) are allowed".
    trials.push(trial_clean_db(&format!("{PREFIX}::037_04_01::without_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-detailed-information.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(
                &QueryParams::new().id(&id).type_("https://ngsi-ld-test-suite/context#Building"),
                RequestHeaders::new(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-04.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_05: Query matching EntityInfo of RegistrationInfo ─────────

    // 037_05_01: Registration with EntityInfo matching query → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_05_01::with_entity_info"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("OffStreetParking").id_pattern(".*downtown$"), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-05-01.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 037_05_02: Registration without EntityInfo → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_05_02::without_entity_info"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_05_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration-with-only-properties-information.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("OffStreetParking").id_pattern(".*downtown$"), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-05-02.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_06: Query matching properties/relationships ───────────────

    // 037_06_01: Query with attrs=name,locatedAt → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_06_01::with_attrs"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Building").attrs(["name", "locatedAt"]), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-06.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 037_06_02: Query with type=Building, no attrs → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_06_02::without_attrs"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_06_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Building"), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-06.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_07: Query matching geoquery ───────────────────────────────

    // 037_07_01: Near point → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_07_01::near_point"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_07_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration-location.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(
                &QueryParams::new()
                    .type_("Building")
                    .georel("near;maxDistance==2000")
                    .geometry("Point")
                    .coordinates("[-8.503,41.202]"),
                link_context_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-07.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 037_07_02: Within polygon → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_07_02::within_polygon"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_07_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration-location.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(
                &QueryParams::new()
                    .type_("Building")
                    .georel("within")
                    .geometry("Polygon")
                    .coordinates("[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-15.23,21.37]]")
                    .geoproperty("location"),
                link_context_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-07.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_08: Query without temporal query ──────────────────────────

    // 037_08_01: No temporal query, type=Vehicle → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_08_01::without_temporal_query"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_08_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Vehicle"), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-08.json");
        expected[0]["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_09: Query with temporal queries ───────────────────────────

    // 037_09_01: observationInterval with observedAt → 200 (5.10.2)
    trials.push(trial_clean_db(
        &format!("{PREFIX}::037_09_01::observation_interval_observed_at"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_09_01:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let reg = fixtures::load_fixture("csource_registrations/context-source-registration-observationInterval.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .query_registrations(
                    &QueryParams::new()
                        .type_("Building")
                        .timeproperty("observedAt")
                        .timerel("before")
                        .timeat("2021-08-01T22:00:00Z"),
                    link_context_headers(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);

            let ignored = &["@context", "createdAt", "modifiedAt", "status"];
            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-09-01.json");
            expected[0]["id"] = serde_json::Value::String(id.clone());
            strip_ignored_keys(&mut actual, ignored);
            strip_ignored_keys(&mut expected, ignored);
            assert_eq!(expected, actual);
        },
    ));

    // 037_09_02: observationInterval without timeproperty → 200 (5.10.2)
    trials.push(trial_clean_db(
        &format!("{PREFIX}::037_09_02::observation_interval_no_timeproperty"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_09_02:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let reg = fixtures::load_fixture("csource_registrations/context-source-registration-observationInterval.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .query_registrations(
                    &QueryParams::new().type_("Building").timerel("before").timeat("2021-08-01T22:00:00Z"),
                    link_context_headers(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);

            let ignored = &["@context", "createdAt", "modifiedAt", "status"];
            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-09-01.json");
            expected[0]["id"] = serde_json::Value::String(id.clone());
            strip_ignored_keys(&mut actual, ignored);
            strip_ignored_keys(&mut expected, ignored);
            assert_eq!(expected, actual);
        },
    ));

    // 037_09_03: managementInterval with createdAt → 200 (5.10.2)
    trials.push(trial_clean_db(
        &format!("{PREFIX}::037_09_03::management_interval_created_at"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_09_03:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let reg = fixtures::load_fixture("csource_registrations/context-source-registration-managementInterval.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .query_registrations(
                    &QueryParams::new()
                        .type_("Building")
                        .timeproperty("createdAt")
                        .timerel("before")
                        .timeat("2021-08-01T22:00:00Z"),
                    link_context_headers(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);

            let ignored = &["@context", "createdAt", "modifiedAt", "status"];
            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-09-02.json");
            expected[0]["id"] = serde_json::Value::String(id.clone());
            strip_ignored_keys(&mut actual, ignored);
            strip_ignored_keys(&mut expected, ignored);
            assert_eq!(expected, actual);
        },
    ));

    // 037_09_04: managementInterval with modifiedAt → 200 (5.10.2)
    trials.push(trial_clean_db(
        &format!("{PREFIX}::037_09_04::management_interval_modified_at"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:037_09_04:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let reg = fixtures::load_fixture("csource_registrations/context-source-registration-managementInterval.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .query_registrations(
                    &QueryParams::new()
                        .type_("Building")
                        .timeproperty("modifiedAt")
                        .timerel("before")
                        .timeat("2021-08-01T22:00:00Z"),
                    link_context_headers(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);

            let ignored = &["@context", "createdAt", "modifiedAt", "status"];
            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-09-02.json");
            expected[0]["id"] = serde_json::Value::String(id.clone());
            strip_ignored_keys(&mut actual, ignored);
            strip_ignored_keys(&mut expected, ignored);
            assert_eq!(expected, actual);
        },
    ));

    // ── 037_10: Query with entity IDs and query conditions ────────────

    // 037_10_01: Query with entity IDs → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_10_01::with_entity_ids"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_10_01_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_10_01_b:{}", uuid::Uuid::new_v4());
        let id3 = format!("urn:ngsi-ld:ContextSourceRegistration:037_10_01_c:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);
        ctx.track_registration(&id3);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);
        let reg3 = fixtures::load_fixture("csource_registrations/context-source-registration-csourceProperty.jsonld", &id3);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg3).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().id(&format!("{id1},{id3}")), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let actual: serde_json::Value = resp.json();
        let arr = actual.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 2, "should return 2 registrations");

        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-10-01.json");
        expected[0]["id"] = serde_json::Value::String(id1.clone());
        expected[1]["id"] = serde_json::Value::String(id3.clone());

        let mut actual_arr: Vec<serde_json::Value> = arr.clone();
        actual_arr.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));
        let mut expected_arr: Vec<serde_json::Value> = expected.as_array().unwrap().clone();
        expected_arr.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));

        for v in &mut actual_arr {
            strip_ignored_keys(v, ignored);
        }
        for v in &mut expected_arr {
            strip_ignored_keys(v, ignored);
        }
        assert_eq!(expected_arr, actual_arr);
    }));

    // 037_10_02: Query with q condition → 200 (5.10.2)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_10_02::with_q_condition"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_10_02_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_10_02_b:{}", uuid::Uuid::new_v4());
        let id3 = format!("urn:ngsi-ld:ContextSourceRegistration:037_10_02_c:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);
        ctx.track_registration(&id3);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id2);
        let reg3 = fixtures::load_fixture("csource_registrations/context-source-registration-csourceProperty.jsonld", &id3);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg3).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().q("csourceProperty1==\"aValue\""), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registrations-037-10-02.json");
        expected[0]["id"] = serde_json::Value::String(id3.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 037_11: Query with pagination ─────────────────────────────────

    // 037_11_01: limit=1, offset=1 → 1 result (5.10.2, 5.5.9)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_11_01::pagination_second"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_01_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_01_b:{}", uuid::Uuid::new_v4());
        let id3 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_01_c:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);
        ctx.track_registration(&id3);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-location.jsonld", &id2);
        let reg3 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id3);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg3).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Building").limit(1).offset(1), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1, "should return 1 registration");
    }));

    // 037_11_02: limit=1, offset=0 → 1 result (5.10.2, 5.5.9)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_11_02::pagination_first"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_02_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_02_b:{}", uuid::Uuid::new_v4());
        let id3 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_02_c:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);
        ctx.track_registration(&id3);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-location.jsonld", &id2);
        let reg3 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id3);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg3).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Building").limit(1).offset(0), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1, "should return 1 registration");
    }));

    // 037_11_03: limit=15, offset=0 → 2 results (5.10.2, 5.5.9)
    trials.push(trial_clean_db(&format!("{PREFIX}::037_11_03::pagination_all"), |ctx| async move {
        let id1 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_03_a:{}", uuid::Uuid::new_v4());
        let id2 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_03_b:{}", uuid::Uuid::new_v4());
        let id3 = format!("urn:ngsi-ld:ContextSourceRegistration:037_11_03_c:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id1);
        ctx.track_registration(&id2);
        ctx.track_registration(&id3);

        let reg1 = fixtures::load_fixture("csource_registrations/context-source-registration.jsonld", &id1);
        let reg2 = fixtures::load_fixture("csource_registrations/context-source-registration-location.jsonld", &id2);
        let reg3 = fixtures::load_fixture("csource_registrations/context-source-registration-detailed-information.jsonld", &id3);

        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_registration(&reg3).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .query_registrations(&QueryParams::new().type_("Building").limit(15).offset(0), link_context_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 2, "should return 2 registrations (only Building type matches)");
    }));
}
