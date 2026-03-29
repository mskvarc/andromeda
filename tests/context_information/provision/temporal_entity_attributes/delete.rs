use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::temporal_attributes::delete";

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
            arr.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        }
        _ => {}
    }
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 015_01: Delete attribute from temporal entity ────────────────────

    // 015_01_01: Delete an attribute from a temporal representation of an entity without deleteAll/datasetId
    trials.push(trial(&format!("{PREFIX}::015_01_01::delete_temporal_attr"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:015_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_temporal_attr(&id, "fuelLevel").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete attr should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-delete-fuelLevel.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        assert_eq!(expected, actual);
    }));

    // 015_01_02: Delete an attribute from a temporal representation of an entity with datasetId
    trials.push(trial(&format!("{PREFIX}::015_01_02::delete_temporal_attr_with_dataset_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:015_01_02:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(
                Method::DELETE,
                &format!("/temporal/entities/{id}/attrs/fuelLevel?datasetId=urn:ngsi-ld:Vehicle:12345-fuel"),
                None,
                Default::default(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete attr with datasetId should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-temporal-representation-delete-fuelLevel-datasetid.jsonld",
            &id,
        );
        normalize(&mut actual);
        normalize(&mut expected);
        assert_eq!(expected, actual);
    }));

    // 015_01_03: Delete an attribute from a temporal representation of an entity with deleteAll
    trials.push(trial(&format!("{PREFIX}::015_01_03::delete_temporal_attr_delete_all"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:015_01_03:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(
                Method::DELETE,
                &format!("/temporal/entities/{id}/attrs/fuelLevel?deleteAll=true"),
                None,
                Default::default(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete attr with deleteAll should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-deleteall-fuelLevel.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        assert_eq!(expected, actual);
    }));

    // ── 015_02: Invalid entity/attribute ID ──────────────────────────────

    // 015_02_01: Delete an attribute from a temporal entity with an empty entity ID
    trials.push(trial(&format!("{PREFIX}::015_02_01::delete_temporal_attr_empty_entity_id"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::DELETE, "/temporal/entities//attrs/fuelLevel", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("BadRequestData"),
            "error type should be BadRequestData"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // 015_02_02: Delete an attribute from a temporal entity with an invalid entity ID
    trials.push(trial(
        &format!("{PREFIX}::015_02_02::delete_temporal_attr_invalid_entity_id"),
        |ctx| async move {
            let resp = ctx.client.delete_temporal_attr("invalidId", "speed").await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("BadRequestData"),
                "error type should be BadRequestData"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 015_02_03: Delete an attribute from a temporal entity with an invalid attribute name
    trials.push(trial(
        &format!("{PREFIX}::015_02_03::delete_temporal_attr_invalid_attr_name"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:015_02_03:{}", uuid::Uuid::new_v4());
            ctx.track_temporal_entity(&id);

            let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
            let resp = ctx.client.create_temporal_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.delete_temporal_attr(&id, "invalid(Name").await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid attr name should return 400");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("BadRequestData"),
                "error type should be BadRequestData"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // ── 015_03: Entity/attribute not found ───────────────────────────────

    // 015_03_01: Delete an attribute from a temporal entity if the entity ID does not exist
    trials.push(trial(
        &format!("{PREFIX}::015_03_01::delete_temporal_attr_entity_not_found"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:015_03_01:{}", uuid::Uuid::new_v4());
            let resp = ctx.client.delete_temporal_attr(&id, "fuelLevel").await;
            assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent entity should return 404");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
                "error type should be ResourceNotFound"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 015_03_02: Delete an attribute from a temporal entity if the entity does not contain the target attribute
    trials.push(trial(&format!("{PREFIX}::015_03_02::delete_temporal_attr_not_found"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:015_03_02:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_temporal_attr(&id, "nonExistentAttr").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent attr should return 404");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
            "error type should be ResourceNotFound"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));
}
