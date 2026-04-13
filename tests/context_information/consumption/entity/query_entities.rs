#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    TestCtx,
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::{trial, trial_clean_db},
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::entity::query";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

fn accept_geojson_headers() -> RequestHeaders {
    RequestHeaders::new().accept_geo_json().link_context(fixtures::test_suite_context_url())
}

/// Mutate a fixture's relationship target.
fn set_relationship_object(entity: &mut serde_json::Value, attr: &str, target: &str) {
    entity[attr]["object"] = serde_json::Value::String(target.to_string());
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 019_01: Query entities ───────────────────────────────────────────

    // 019_01_01: Query several entities based on ids
    trials.push(trial(&format!("{PREFIX}::019_01_01::by_ids"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_01_01b");
        let id3 = ctx.entity_id("urn:ngsi-ld:Building:019_01_01c");

        for id in [&id1, &id2, &id3] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let params = QueryParams::new().id(format!("{id1},{id2}")).type_("Building");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2, "should return exactly 2 entities");
    }));

    // 019_01_02: Query several entities based on the entity types
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_02::by_type"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:019_01_02");
        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:019_01_02");
        let parking_id = ctx.entity_id("urn:ngsi-ld:OffStreetParking:019_01_02");

        let building = fixtures::load_fixture("entities/building-minimal.jsonld", &building_id);
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &vehicle_id);
        let resp = ctx.client.create_entity(&vehicle).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let parking = fixtures::load_fixture("entities/parking-simple-attributes.jsonld", &parking_id);
        let resp = ctx.client.create_entity(&parking).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle,OffStreetParking");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2, "should return vehicle and parking");
    }));

    // 019_01_03: Query several entities based on the given id pattern
    trials.push(trial(&format!("{PREFIX}::019_01_03::by_id_pattern"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_01_03a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_01_03b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let params = QueryParams::new().id_pattern("urn:ngsi-ld:Building:019_01_03.*").type_("Building");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }));

    // 019_01_04: Query several entities based on attribute names
    trials.push(trial(&format!("{PREFIX}::019_01_04::by_attrs"), |ctx| async move {
        let compound = fixtures::compound_context_url();

        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_01_04a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_01_04b");

        let mut e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        e1["@context"] = serde_json::json!([compound]);
        let resp = ctx.client.create_entity(&e1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        e2["@context"] = serde_json::json!([compound]);
        let resp = ctx.client.create_entity(&e2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().attrs([
            "https://ngsi-ld-test-suite/context#airQualityLevel",
            "https://ngsi-ld-test-suite/context#subCategory",
        ]);
        let headers = RequestHeaders::new().accept_ld_json().link_context(compound);
        let resp = ctx.client.query_entities(&params, headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 1, "only entity with matching attrs should be returned");
        pretty_assertions::assert_eq!(arr[0]["id"], id1);
    }));

    // 019_01_05: Query several entities based on a geoquery
    trials.push(trial(&format!("{PREFIX}::019_01_05::by_geoquery"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_01_05a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_01_05b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-location-attribute.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let params = QueryParams::new()
            .type_("Building")
            .georel("equals")
            .geometry("Point")
            .coordinates("[13.3986,52.5547]");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2, "both entities share the same location");
    }));

    // ── 019_01_06: Scope queries ─────────────────────────────────────────

    // 019_01_06: Query with full scope
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_06a::scope_full"), |ctx| async move {
        let compound = fixtures::compound_context_url();
        let (id1, id2) = setup_scope_entities(&ctx, compound).await;

        let count = query_scope_count(&ctx, compound, "/Madrid/Gardens/ParqueNorte").await;
        pretty_assertions::assert_eq!(count, 2);
        let _ = (id1, id2);
    }));

    // 019_01_06: Query with plus matching
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_06b::scope_plus"), |ctx| async move {
        let compound = fixtures::compound_context_url();
        let (id1, id2) = setup_scope_entities(&ctx, compound).await;

        let count = query_scope_count(&ctx, compound, "/Madrid/+/ParqueNorte").await;
        pretty_assertions::assert_eq!(count, 2);
        let _ = (id1, id2);
    }));

    // 019_01_06: Query with hash matching
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_06c::scope_hash"), |ctx| async move {
        let compound = fixtures::compound_context_url();
        let (id1, id2) = setup_scope_entities(&ctx, compound).await;

        let count = query_scope_count(&ctx, compound, "/CompanyA/#").await;
        pretty_assertions::assert_eq!(count, 1);
        let _ = (id1, id2);
    }));

    // 019_01_06: Query non-empty scope
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_06d::scope_nonempty"), |ctx| async move {
        let compound = fixtures::compound_context_url();
        let (id1, id2) = setup_scope_entities(&ctx, compound).await;

        let count = query_scope_count(&ctx, compound, "/#").await;
        pretty_assertions::assert_eq!(count, 2);
        let _ = (id1, id2);
    }));

    // 019_01_06: Query with AND scope
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_06e::scope_and"), |ctx| async move {
        let compound = fixtures::compound_context_url();
        let (id1, id2) = setup_scope_entities(&ctx, compound).await;

        let count = query_scope_count(&ctx, compound, "/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC").await;
        pretty_assertions::assert_eq!(count, 1);
        let _ = (id1, id2);
    }));

    // 019_01_06: Query with OR scope
    trials.push(trial_clean_db(&format!("{PREFIX}::019_01_06f::scope_or"), |ctx| async move {
        let compound = fixtures::compound_context_url();
        let (id1, id2) = setup_scope_entities(&ctx, compound).await;

        let count = query_scope_count(&ctx, compound, "/Madrid/Gardens/ParqueNorte,/CompanyA/OrganizationB/UnitC").await;
        pretty_assertions::assert_eq!(count, 2);
        let _ = (id1, id2);
    }));

    // ── 019_02: POST query ──────────────────────────────────────────────

    // 019_02_01: Query one entity via POST interaction based on id
    trials.push(trial(&format!("{PREFIX}::019_02_01::post_query_by_id"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_02_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_02_01b");
        let id3 = ctx.entity_id("urn:ngsi-ld:Building:019_02_01c");

        for id in [&id1, &id2, &id3] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let query_body = serde_json::json!({
            "entities": [{"id": &id1, "type": "Building"}],
            "@context": fixtures::test_suite_context_url()
        });
        let resp = ctx.client.batch_query(&query_body, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 1);
        pretty_assertions::assert_eq!(arr[0]["id"], id1);
    }));

    // 019_02_02: Query several entities via POST interaction based on the entity types
    trials.push(trial(&format!("{PREFIX}::019_02_02::post_query_by_type"), |ctx| async move {
        let compound = fixtures::compound_context_url();

        let building_id = ctx.entity_id("urn:ngsi-ld:Building:019_02_02");
        let vehicle_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:019_02_02a");
        let vehicle_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:019_02_02b");

        let mut building = fixtures::load_fixture("entities/building-minimal.jsonld", &building_id);
        building["@context"] = serde_json::json!([compound]);
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        for id in [&vehicle_id1, &vehicle_id2] {
            let mut vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", id);
            vehicle["@context"] = serde_json::json!([compound]);
            let resp = ctx.client.create_entity(&vehicle).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let query_body = serde_json::json!({
            "entities": [{"type": "Vehicle"}],
            "@context": compound
        });
        let resp = ctx.client.batch_query(&query_body, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }));

    // 019_02_03: Query several entities via POST interaction based on the given id pattern
    trials.push(trial(&format!("{PREFIX}::019_02_03::post_query_by_id_pattern"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_02_03a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_02_03b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let query_body = serde_json::json!({
            "entities": [{"idPattern": "urn:ngsi-ld:Building:019_02_03.*", "type": "Building"}],
            "@context": fixtures::test_suite_context_url()
        });
        let resp = ctx.client.batch_query(&query_body, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }));

    // 019_02_04: Query several entities via POST interaction based on attribute names
    trials.push(trial(&format!("{PREFIX}::019_02_04::post_query_by_attrs"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:019_02_04");
        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:019_02_04");

        let building = fixtures::load_fixture("entities/building-minimal.jsonld", &building_id);
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &vehicle_id);
        let resp = ctx.client.create_entity(&vehicle).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let query_body = serde_json::json!({
            "attrs": ["brandName", "isParked"],
            "@context": fixtures::test_suite_context_url()
        });
        let resp = ctx.client.batch_query(&query_body, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 1, "only vehicle has brandName/isParked");
    }));

    // 019_02_05: Query several entities via POST interaction asking for a GeoJSON representation
    trials.push(trial(&format!("{PREFIX}::019_02_05::post_query_geojson"), |ctx| async move {
        let compound = fixtures::compound_context_url();

        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:019_02_05");
        let parking_id = ctx.entity_id("urn:ngsi-ld:OffStreetParking:019_02_05");

        let mut vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &vehicle_id);
        vehicle["@context"] = serde_json::json!([compound]);
        let resp = ctx.client.create_entity(&vehicle).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut parking = fixtures::load_fixture("entities/parking-simple-attributes.jsonld", &parking_id);
        parking["@context"] = serde_json::json!([compound]);
        let resp = ctx.client.create_entity(&parking).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let query_body = serde_json::json!({
            "type": "Query",
            "entities": [{"type": "Vehicle"}, {"type": "OffStreetParking"}]
        });
        let headers = RequestHeaders::new().accept_geo_json().link_context(compound);
        let resp = ctx.client.batch_query(&query_body, headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["type"], "FeatureCollection");
    }));

    // 019_02_06: Query two entities via POST interaction based on ids
    trials.push(trial(&format!("{PREFIX}::019_02_06::post_query_two_ids"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_02_06a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_02_06b");
        let id3 = ctx.entity_id("urn:ngsi-ld:Building:019_02_06c");

        for id in [&id1, &id2, &id3] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let query_body = serde_json::json!({
            "entities": [
                {"id": &id1, "type": "Building"},
                {"id": &id2, "type": "Building"}
            ],
            "@context": fixtures::test_suite_context_url()
        });
        let resp = ctx.client.batch_query(&query_body, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }));

    // ── 019_03: Invalid query params ─────────────────────────────────────

    // 019_03_01: Query entities based on incorrect ids
    trials.push(trial(&format!("{PREFIX}::019_03_01::invalid_ids"), |ctx| async move {
        let params = QueryParams::new().id("thisisaninvaliduri1,thisisaninvaliduri2").type_("Building");
        let resp = ctx.client.query_entities(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
    }));

    // 019_03_02: Query several entities based on incorrect id pattern
    trials.push(trial(&format!("{PREFIX}::019_03_02::invalid_id_pattern"), |ctx| async move {
        let params = QueryParams::new().id_pattern("invalid_entity_id_pattern**").type_("Building");
        let resp = ctx.client.query_entities(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
    }));

    // 019_03_03: Query several entities based on incorrect attribute names
    trials.push(trial(&format!("{PREFIX}::019_03_03::invalid_attrs"), |ctx| async move {
        let params = QueryParams::new().attrs(["id", "type"]);
        let resp = ctx.client.query_entities(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
    }));

    // 019_03_04: Query entities when the request has a wrong geometryProperty
    trials.push(trial(&format!("{PREFIX}::019_03_04::invalid_georel"), |ctx| async move {
        let params = QueryParams::new()
            .type_("Building")
            .georel("invalid")
            .geometry("Point")
            .coordinates("[-8.503,41.202]")
            .geoproperty("location");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("BadRequestData"));
    }));

    // ── 019_04: Simplified representation ────────────────────────────────

    // 019_04_01: Query entities in a simplified representation
    trials.push(trial(&format!("{PREFIX}::019_04_01::key_values"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_04_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_04_01b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let params = QueryParams::new()
            .id(format!("{id1},{id2}"))
            .type_("Building")
            .options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
        for entity in arr {
            assert!(entity["name"].is_string(), "name should be simplified");
        }
    }));

    // ── 019_05: GeoJSON query ────────────────────────────────────────────

    // 019_05_01: Get entities that can be returned in GeoJSON format
    trials.push(trial_clean_db(&format!("{PREFIX}::019_05_01::geojson"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_05_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_05_01b");

        let e1 = fixtures::load_fixture("entities/building-location-attribute.jsonld", &id1);
        let resp = ctx.client.create_entity(&e1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let e2 = fixtures::load_fixture("entities/building-location-attribute.jsonld", &id2);
        let resp = ctx.client.create_entity(&e2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().id(format!("{id1},{id2}")).type_("Building");
        let resp = ctx.client.query_entities(&params, accept_geojson_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["type"], "FeatureCollection");
        assert!(body["features"].as_array().is_some());
    }));

    // ── 019_06: Pagination ───────────────────────────────────────────────

    // 019_06_01: Query entities specifying a maximum number of results
    trials.push(trial_clean_db(&format!("{PREFIX}::019_06_01::with_limit"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_06_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_06_01b");
        let id3 = ctx.entity_id("urn:ngsi-ld:Building:019_06_01c");

        for id in [&id1, &id2, &id3] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let params = QueryParams::new().id(format!("{id1},{id2},{id3}")).type_("Building").limit(2);
        let resp = ctx.client.query_entities(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2, "should return only 2 entities with limit=2");
    }));

    // ── 019_07: Count header ─────────────────────────────────────────────

    // 019_07_01: Check count header with entity type only
    trials.push(trial_clean_db(&format!("{PREFIX}::019_07_01::count_type_only"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_07_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_07_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        let resp = ctx.client.create_entity(&e1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let e2 = fixtures::load_fixture("entities/building-simple-attributes-second.jsonld", &id2);
        let resp = ctx.client.create_entity(&e2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").count(true);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = resp
            .headers
            .get("NGSILD-Results-Count")
            .expect("should have NGSILD-Results-Count header")
            .to_str()
            .unwrap();
        pretty_assertions::assert_eq!(count, "2");
    }));

    // 019_07_02: Check count header with q parameter
    trials.push(trial_clean_db(&format!("{PREFIX}::019_07_02::count_with_q"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_07_02a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_07_02b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        let resp = ctx.client.create_entity(&e1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let e2 = fixtures::load_fixture("entities/building-simple-attributes-second.jsonld", &id2);
        let resp = ctx.client.create_entity(&e2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").q("airQualityLevel==6").count(true);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = resp
            .headers
            .get("NGSILD-Results-Count")
            .expect("should have NGSILD-Results-Count header")
            .to_str()
            .unwrap();
        pretty_assertions::assert_eq!(count, "1");
    }));

    // ── 019_08: Entity Type Selection Language ────────────────────────────

    for (suffix, label, type_str, expected_count, check_ids) in [
        ("019_08_01", "one_type", "Building", 2u64, true),
        ("019_08_02", "and_operator", "(Building;TouristDestination)", 1, false),
        ("019_08_03", "or_operator", "Building,Parking", 3, false),
        ("019_08_04", "or_different", "Parking,TouristDestination", 2, true),
        ("019_08_05", "two_operators", "(Building;Parking),TouristDestination", 1, false),
    ] {
        trials.push(trial_clean_db(&format!("{PREFIX}::{suffix}::type_selection_{label}"), move |ctx| async move {
            let id1 = ctx.entity_id("urn:ngsi-ld:MultiTypes:019_08a");
            let id2 = ctx.entity_id("urn:ngsi-ld:MultiTypes:019_08b");
            let id3 = ctx.entity_id("urn:ngsi-ld:MultiTypes:019_08c");

            let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
            let resp = ctx.client.create_entity(&e1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e2 = fixtures::load_fixture("entities/building-with-different-type.jsonld", &id2);
            let resp = ctx.client.create_entity(&e2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e3 = fixtures::load_fixture("entities/building-with-two-types.jsonld", &id3);
            let resp = ctx.client.create_entity(&e3).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().type_(type_str);
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            let arr = body.as_array().expect("should return array");
            pretty_assertions::assert_eq!(arr.len() as u64, expected_count);

            let _ = (check_ids, id1, id2, id3);
        }));
    }

    // ── 019_09: Query with q parameter ───────────────────────────────────

    for (suffix, label, q, expected_count) in [
        ("019_09_01", "name", "name==%22Eiffel Tower%22", 1u64),
        ("019_09_02", "name_and_almost_full", "name==%22Pisa Tower%22;almostFull==false", 1),
        ("019_09_03", "name_and_almost_full_dup", "name==%22Pisa Tower%22;almostFull==false", 1),
        ("019_09_04", "name_or_almost_full", "name==%22Pisa Tower%22|almostFull==false", 3),
        (
            "019_09_05",
            "names_and_almost_full",
            "(name==%22Pisa Tower%22|name==%22Eiffel Tower%22);almostFull==false",
            2,
        ),
        ("019_09_06", "air_quality_level", "airQualityLevel==4", 1),
        ("019_09_07", "air_quality_range", "airQualityLevel>=4;airQualityLevel<7", 2),
        ("019_09_08", "located_in_existing", "locatedIn==urn:ngsi-ld:City:Pisa", 1),
        ("019_09_09", "located_in_nonexisting", "locatedIn==urn:ngsi-ld:City:Paris", 0),
        (
            "019_09_10",
            "located_in_two",
            "locatedIn==urn:ngsi-ld:City:Pisa|locatedIn==urn:ngsi-ld:City:Paris",
            1,
        ),
    ] {
        trials.push(trial_clean_db(&format!("{PREFIX}::{suffix}::q_{label}"), move |ctx| async move {
            let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_09a");
            let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_09b");
            let id3 = ctx.entity_id("urn:ngsi-ld:Building:019_09c");

            let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
            let resp = ctx.client.create_entity(&e1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e2 = fixtures::load_fixture("entities/building-simple-attributes-second.jsonld", &id2);
            let resp = ctx.client.create_entity(&e2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e3 = fixtures::load_fixture("entities/building-simple-attributes-third.jsonld", &id3);
            let resp = ctx.client.create_entity(&e3).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().q(q).count(true);
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
            let count: u64 = resp
                .headers
                .get("NGSILD-Results-Count")
                .expect("should have count header")
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            pretty_assertions::assert_eq!(count, expected_count);
        }));
    }

    // ── 019_10: Geoquery with Point ──────────────────────────────────────

    for (suffix, label, georel, geometry, coordinates, expected_count) in [
        ("019_10_01", "equals_point", "equals", "Point", "[13.3986,52.5547]", 1u64),
        ("019_10_02", "near_max_distance", "near;maxDistance==15000", "Point", "[13.388930,52.534473]", 2),
        ("019_10_03", "near_min_distance", "near;minDistance==15000", "Point", "[13.388930,52.534473]", 1),
        (
            "019_10_04",
            "within_polygon",
            "within",
            "Polygon",
            "[[[13.1616211,52.5438124],[13.597641,52.4840224],[13.5948944,52.3402782],[13.142395,52.386428],[13.1616211,52.5438124]]]",
            2,
        ),
        (
            "019_10_05",
            "disjoint_polygon",
            "disjoint",
            "Polygon",
            "[[[13.1616211,52.5438124],[13.597641,52.4840224],[13.5948944,52.3402782],[13.142395,52.386428],[13.1616211,52.5438124]]]",
            1,
        ),
        ("019_10_06", "disjoint_point", "disjoint", "Point", "[13.3986,52.5547]", 2),
        ("019_10_07", "contains_point", "contains", "Point", "[13.3986,52.5547]", 1),
        ("019_10_08", "intersects_point", "intersects", "Point", "[13.3986,52.5547]", 1),
    ] {
        trials.push(trial_clean_db(&format!("{PREFIX}::{suffix}::geo_{label}"), move |ctx| async move {
            let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_10a");
            let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_10b");
            let id3 = ctx.entity_id("urn:ngsi-ld:Building:019_10c");

            let e1 = fixtures::load_fixture("entities/building-location-attribute.jsonld", &id1);
            let resp = ctx.client.create_entity(&e1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e2 = fixtures::load_fixture("entities/building-location-attribute-second.jsonld", &id2);
            let resp = ctx.client.create_entity(&e2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e3 = fixtures::load_fixture("entities/building-location-attribute-third.jsonld", &id3);
            let resp = ctx.client.create_entity(&e3).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new()
                .type_("Building")
                .georel(georel)
                .geometry(geometry)
                .coordinates(coordinates)
                .count(true);
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
            let count: u64 = resp
                .headers
                .get("NGSILD-Results-Count")
                .expect("should have count header")
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            pretty_assertions::assert_eq!(count, expected_count);
        }));
    }

    // ── 019_11: Geoquery with Polygon ────────────────────────────────────

    for (suffix, label, georel, geometry, coordinates, expected_count) in [
        (
            "019_11_01",
            "equals_polygon",
            "equals",
            "Polygon",
            "[[[13.2865906,52.5648645],[13.2879639,52.5648645],[13.2797241,52.4988679],[13.477478,52.4712703],[13.5049438,52.5373084],[13.2865906,52.5648645]]]",
            1u64,
        ),
        (
            "019_11_02",
            "intersects_polygon",
            "intersects",
            "Polygon",
            "[[[13.3456421,52.4794848],[13.3641815,52.4428773],[13.4177399,52.4730023],[13.3459854,52.479903],[13.3456421,52.4794848]]]",
            1,
        ),
        (
            "019_11_03",
            "contains_polygon_matching",
            "contains",
            "Polygon",
            "[[[13.3844376,52.5356966],[13.3844376,52.5356966],[13.381691,52.5123012],[13.4235764,52.5100027],[13.4331894,52.5273425],[13.3844376,52.5356966]]]",
            1,
        ),
        (
            "019_11_04",
            "contains_polygon_not_matching",
            "contains",
            "Polygon",
            "[[[13.1369019,52.5198225],[13.1365585,52.5200314],[13.131752,52.4980907],[13.1832504,52.5026887],[13.1369019,52.5198225]]]",
            0,
        ),
        (
            "019_11_05",
            "disjoint_polygon",
            "disjoint",
            "Polygon",
            "[[[13.1369019,52.5198225],[13.1365585,52.5200314],[13.131752,52.4980907],[13.1832504,52.5026887],[13.1369019,52.5198225]]]",
            2,
        ),
        (
            "019_11_06",
            "within_polygon",
            "within",
            "Polygon",
            "[[[13.3023834,52.4903066],[13.3027267,52.4905156],[13.2745743,52.465632],[13.3343124,52.4185447],[13.4311295,52.47107],[13.3023834,52.4903066]]]",
            1,
        ),
        (
            "019_11_07",
            "overlaps_polygon",
            "overlaps",
            "Polygon",
            "[[[13.3456421,52.4794848],[13.3641815,52.4428773],[13.4177399,52.4730023],[13.3459854,52.479903],[13.3456421,52.4794848]]]",
            1,
        ),
        ("019_11_08", "contains_point_in_polygon", "contains", "Point", "[13.3442688,52.4650045]", 1),
    ] {
        trials.push(trial_clean_db(&format!("{PREFIX}::{suffix}::geo_polygon_{label}"), move |ctx| async move {
            let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_11a");
            let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_11b");

            let e1 = fixtures::load_fixture("entities/building-location-polygon.jsonld", &id1);
            let resp = ctx.client.create_entity(&e1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let e2 = fixtures::load_fixture("entities/building-location-polygon-second.jsonld", &id2);
            let resp = ctx.client.create_entity(&e2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new()
                .type_("Building")
                .georel(georel)
                .geometry(geometry)
                .coordinates(coordinates)
                .count(true);
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
            let count: u64 = resp
                .headers
                .get("NGSILD-Results-Count")
                .expect("should have count header")
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            pretty_assertions::assert_eq!(count, expected_count);
        }));
    }

    // ── 019_12: Dataset ID filter ────────────────────────────────────────

    for (suffix, label, attrs, dataset_id) in [
        ("019_12_01", "attrs_only", Some(vec!["name"]), None),
        ("019_12_02", "id_only", None, Some("urn:ngsi-ld:Dataset:french-name")),
        (
            "019_12_03",
            "two_ids",
            None,
            Some("urn:ngsi-ld:Dataset:french-name,urn:ngsi-ld:Dataset:spanish-name"),
        ),
        ("019_12_04", "default_instance", None, Some("@none")),
        ("019_12_05", "attrs_default", Some(vec!["name"]), Some("@none")),
        ("019_12_06", "attrs_and_id", Some(vec!["name"]), Some("urn:ngsi-ld:Dataset:german-name")),
        ("019_12_07", "attrs_no_match", Some(vec!["name"]), Some("urn:ngsi-ld:Dataset:spanish-name")),
    ] {
        trials.push(trial_clean_db(&format!("{PREFIX}::{suffix}::dataset_{label}"), move |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:019_12");
            let entity = fixtures::load_fixture("entities/building-multi-instances-attributes.jsonld", &id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let mut params = QueryParams::new().type_("Building");
            if let Some(a) = attrs {
                params = params.attrs(a.iter().map(|s| s.to_string()));
            }
            if let Some(d) = dataset_id {
                params = params.dataset_id(d);
            }
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            let arr = body.as_array().expect("should return array");
            pretty_assertions::assert_eq!(arr.len(), 1);
        }));
    }

    // ── 019_13: One-level linked entities in query ───────────────────────

    // 019_13_01: Query inline linked (normalized)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_13_01::inline_linked"), |ctx| async move {
        let ids = setup_one_level_linked_query(&ctx).await;

        let params = QueryParams::new().type_("Building").join("inline").join_level(1);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
        let _ = ids;
    }));

    // 019_13_02: Query flat linked (normalized)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_13_02::flat_linked"), |ctx| async move {
        let ids = setup_one_level_linked_query(&ctx).await;

        let params = QueryParams::new().type_("Building").join("flat").join_level(1);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        assert!(arr.len() >= 4, "should contain 2 buildings + 2 cities");
        let _ = ids;
    }));

    // 019_13_03: Query inline linked (simplified)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_13_03::inline_linked_simplified"), |ctx| async move {
        let ids = setup_one_level_linked_query(&ctx).await;

        let params = QueryParams::new()
            .type_("Building")
            .join("inline")
            .join_level(1)
            .options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
        let _ = ids;
    }));

    // 019_13_04: Query flat linked (simplified)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_13_04::flat_linked_simplified"), |ctx| async move {
        let ids = setup_one_level_linked_query(&ctx).await;

        let params = QueryParams::new()
            .type_("Building")
            .join("flat")
            .join_level(1)
            .options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        assert!(arr.len() >= 4, "should contain 2 buildings + 2 cities");
        let _ = ids;
    }));

    // ── 019_14: Inline linked + sysAttrs ─────────────────────────────────

    // 019_14_01: Query entities with inline linked entities and sysAttrs
    trials.push(trial_clean_db(&format!("{PREFIX}::019_14_01::inline_linked_sys_attrs"), |ctx| async move {
        let ids = setup_one_level_linked_query(&ctx).await;

        let params = QueryParams::new()
            .type_("Building")
            .join("inline")
            .join_level(1)
            .options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        for entity in arr {
            assert!(entity["locatedAt"]["entity"].get("createdAt").is_some(), "linked entity should have createdAt");
        }
        let _ = ids;
    }));

    // ── 019_15: Flat linked + sysAttrs ───────────────────────────────────

    // 019_15_01: Query entities with flat linked entities and sysAttrs
    trials.push(trial_clean_db(&format!("{PREFIX}::019_15_01::flat_linked_sys_attrs"), |ctx| async move {
        let ids = setup_one_level_linked_query(&ctx).await;

        let params = QueryParams::new()
            .type_("Building")
            .join("flat")
            .join_level(1)
            .options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        for entity in arr {
            assert!(entity.get("createdAt").is_some(), "all entities should have createdAt");
        }
        let _ = ids;
    }));

    // ── 019_16: Two-level linked entities in query ───────────────────────

    // 019_16_01: Query inline two-level (normalized)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_16_01::inline_two_level"), |ctx| async move {
        let ids = setup_two_level_linked_query(&ctx).await;

        let params = QueryParams::new().type_("Building").join("inline").join_level(2);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
        let _ = ids;
    }));

    // 019_16_02: Query flat two-level (normalized)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_16_02::flat_two_level"), |ctx| async move {
        let ids = setup_two_level_linked_query(&ctx).await;

        let params = QueryParams::new().type_("Building").join("flat").join_level(2);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        assert!(arr.len() >= 6, "should contain buildings, cities, and countries");
        let _ = ids;
    }));

    // 019_16_03: Query inline two-level (simplified)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_16_03::inline_two_level_simplified"), |ctx| async move {
        let ids = setup_two_level_linked_query(&ctx).await;

        let params = QueryParams::new()
            .type_("Building")
            .join("inline")
            .join_level(2)
            .options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        pretty_assertions::assert_eq!(arr.len(), 2);
        let _ = ids;
    }));

    // 019_16_04: Query flat two-level (simplified)
    trials.push(trial_clean_db(&format!("{PREFIX}::019_16_04::flat_two_level_simplified"), |ctx| async move {
        let ids = setup_two_level_linked_query(&ctx).await;

        let params = QueryParams::new()
            .type_("Building")
            .join("flat")
            .join_level(2)
            .options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("should return array");
        assert!(arr.len() >= 6, "should contain buildings, cities, and countries");
        let _ = ids;
    }));
}

// ── Helpers ──────────────────────────────────────────────────────────────

async fn setup_scope_entities(ctx: &TestCtx, compound: &str) -> (String, String) {
    let id1 = ctx.entity_id("urn:ngsi-ld:Building:019_01_06a");
    let e1 = fixtures::load_fixture("entities/building-minimal-with-one-scope.json", &id1);
    let headers = RequestHeaders::new().content_type("application/json").link_context(compound);
    let resp = ctx.client.request(Method::POST, "/entities", Some(&e1), headers).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let id2 = ctx.entity_id("urn:ngsi-ld:Building:019_01_06b");
    let e2 = fixtures::load_fixture("entities/building-minimal-with-many-scopes.json", &id2);
    let headers = RequestHeaders::new().content_type("application/json").link_context(compound);
    let resp = ctx.client.request(Method::POST, "/entities", Some(&e2), headers).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    (id1, id2)
}

async fn query_scope_count(ctx: &TestCtx, compound: &str, scopeq: &str) -> usize {
    let params = QueryParams::new().type_("Building").scopeq(scopeq);
    let headers = RequestHeaders::new().accept_ld_json().link_context(compound);
    let resp = ctx.client.query_entities(&params, headers).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
    let body: serde_json::Value = resp.json();
    body.as_array().expect("should return array").len()
}

async fn setup_one_level_linked_query(ctx: &TestCtx) -> Vec<String> {
    let building_id1 = ctx.entity_id("urn:ngsi-ld:Building:019_13_1");
    let building_id2 = ctx.entity_id("urn:ngsi-ld:Building:019_13_2");
    let city_id1 = ctx.entity_id("urn:ngsi-ld:City:019_13_1");
    let city_id2 = ctx.entity_id("urn:ngsi-ld:City:019_13_2");

    let mut b1 = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id1);
    set_relationship_object(&mut b1, "locatedAt", &city_id1);
    let resp = ctx.client.create_entity(&b1).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let mut b2 = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id2);
    set_relationship_object(&mut b2, "locatedAt", &city_id2);
    let resp = ctx.client.create_entity(&b2).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let c1 = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id1);
    let resp = ctx.client.create_entity(&c1).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let c2 = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id2);
    let resp = ctx.client.create_entity(&c2).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    vec![building_id1, building_id2, city_id1, city_id2]
}

async fn setup_two_level_linked_query(ctx: &TestCtx) -> Vec<String> {
    let building_id1 = ctx.entity_id("urn:ngsi-ld:Building:019_16_1");
    let building_id2 = ctx.entity_id("urn:ngsi-ld:Building:019_16_2");
    let city_id1 = ctx.entity_id("urn:ngsi-ld:City:019_16_1");
    let city_id2 = ctx.entity_id("urn:ngsi-ld:City:019_16_2");
    let country_id1 = ctx.entity_id("urn:ngsi-ld:Country:019_16_1");
    let country_id2 = ctx.entity_id("urn:ngsi-ld:Country:019_16_2");

    let mut b1 = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id1);
    set_relationship_object(&mut b1, "locatedAt", &city_id1);
    let resp = ctx.client.create_entity(&b1).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let mut b2 = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id2);
    set_relationship_object(&mut b2, "locatedAt", &city_id2);
    let resp = ctx.client.create_entity(&b2).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let mut c1 = fixtures::load_fixture("entities/city-relationship.jsonld", &city_id1);
    set_relationship_object(&mut c1, "locatedAt", &country_id1);
    let resp = ctx.client.create_entity(&c1).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let mut c2 = fixtures::load_fixture("entities/city-relationship.jsonld", &city_id2);
    set_relationship_object(&mut c2, "locatedAt", &country_id2);
    let resp = ctx.client.create_entity(&c2).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let co1 = fixtures::load_fixture("entities/country-minimal.jsonld", &country_id1);
    let resp = ctx.client.create_entity(&co1).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let co2 = fixtures::load_fixture("entities/country-minimal.jsonld", &country_id2);
    let resp = ctx.client.create_entity(&co2).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    vec![building_id1, building_id2, city_id1, city_id2, country_id1, country_id2]
}
