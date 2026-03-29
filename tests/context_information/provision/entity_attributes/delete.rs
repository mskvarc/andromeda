use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entity_attributes::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // 013_01_01: Delete attribute default instance
    trials.push(trial(&format!("{PREFIX}::013_01::delete_attribute_default_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_01_01");

        let entity = fixtures::load_fixture("entities/vehicle-speed-only-multi-instances.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx.client.delete_entity_attr(&id, "speed").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("speed").is_some());
    }));

    // 013_01_02: Delete attribute with specific datasetId
    trials.push(trial(&format!("{PREFIX}::013_01::delete_attribute_with_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_01_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request(
                Method::DELETE,
                &format!("/entities/{id}/attrs/speed?datasetId=urn:ngsi-ld:Property:gpsBxyz123-speed"),
                None,
                Default::default(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("speed").is_some());
    }));

    // 013_01_03: Delete all attribute instances (deleteAll=true)
    trials.push(trial(&format!("{PREFIX}::013_01::delete_all_attribute_instances"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_01_03");

        let entity = fixtures::load_fixture("entities/vehicle-speed-only-multi-instances.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request(Method::DELETE, &format!("/entities/{id}/attrs/speed?deleteAll=true"), None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("speed").is_none(), "all speed instances should be deleted");
    }));

    // 013_02_01: Delete with empty entity ID
    trials.push(trial(&format!("{PREFIX}::013_02::delete_empty_entity_id"), |ctx| async move {
        let resp = ctx.client.request(Method::DELETE, "/entities//attrs/speed", None, Default::default()).await;
        assert!(
            resp.status == StatusCode::BAD_REQUEST || resp.status == StatusCode::NOT_FOUND || resp.status == StatusCode::METHOD_NOT_ALLOWED,
            "expected error status, got {}",
            resp.status
        );
    }));

    // 013_02_02: Delete with invalid entity URI
    trials.push(trial(&format!("{PREFIX}::013_02::delete_invalid_entity_id"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::DELETE, "/entities/not-a-valid-uri/attrs/speed", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 013_02_03: Delete with missing attribute name returns 405
    trials.push(trial(&format!("{PREFIX}::013_02::delete_missing_attribute_name"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_02_03");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request(Method::DELETE, &format!("/entities/{id}/attrs"), None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED);
    }));

    // 013_03_01: Delete from non-existent entity returns 404
    trials.push(trial(&format!("{PREFIX}::013_03::delete_nonexistent_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_03_01");
        let resp = ctx.client.delete_entity_attr(&id, "speed").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 013_03_02: Delete non-existent attribute returns 404
    trials.push(trial(&format!("{PREFIX}::013_03::delete_nonexistent_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_03_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx.client.delete_entity_attr(&id, "nonExistentAttr").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 013_03_03: Delete with non-existent datasetId returns 404
    trials.push(trial(&format!("{PREFIX}::013_03::delete_nonexistent_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:013_03_03");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request(
                Method::DELETE,
                &format!("/entities/{id}/attrs/speed?datasetId=urn:ngsi-ld:Dataset:nonexistent"),
                None,
                Default::default(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 013_04: Delete scope attribute
    trials.push(trial(&format!("{PREFIX}::013_04::delete_scope"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:013_04");

        let entity = fixtures::load_fixture("entities/building-minimal-with-one-scope.json", &id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities",
                Some(&entity),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED, "create entity should return 201");

        let resp = ctx.client.delete_entity_attr(&id, "scope").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("scope").is_none(), "scope should be deleted");
    }));

    // 013_05: Delete observationSpace
    trials.push(trial(&format!("{PREFIX}::013_05::delete_observation_space"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:013_05");

        let entity = fixtures::load_fixture("entities/building-observation-space-geoproperty.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx.client.delete_entity_attr(&id, "observationSpace").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("observationSpace").is_none());
    }));

    // 013_06: Delete operationSpace
    trials.push(trial(&format!("{PREFIX}::013_06::delete_operation_space"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:013_06");

        let entity = fixtures::load_fixture("entities/building-operation-space-geoproperty.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx.client.delete_entity_attr(&id, "operationSpace").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("operationSpace").is_none());
    }));

    // 013_07_01: Delete VocabProperty attribute
    trials.push(trial(&format!("{PREFIX}::013_07::delete_vocab_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:013_07_01");

        let entity = fixtures::load_fixture("entities/building-vocab-property-string.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx.client.delete_entity_attr(&id, "vocabProperty").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("vocabProperty").is_none());
    }));
}
