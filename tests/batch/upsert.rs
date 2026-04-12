use reqwest::{Method, StatusCode};
use serial_test::serial;

use crate::helpers::*;
use serde_json::json;

#[tokio::test]
#[serial]
async fn batch_upsert_new_entities_with_properties_returns_201() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:004_01_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:004_01_01b");

    let e1 = building_simple_attributes(&id1);
    let e2 = building_simple_attributes(&id2);
    let entities = json!([e1, e2]);

    let (status, body) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED, "upsert of new entities should return 201");

    let uris = body.as_array().expect("response should be array of URIs");
    assert_eq!(uris.len(), 2);
}

#[tokio::test]
#[serial]
async fn batch_upsert_new_entities_with_relationships_returns_201() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:004_01_02a");
    let id2 = unique_id("urn:ngsi-ld:Building:004_01_02b");

    let e1 = building_relationship(&id1);
    let e2 = building_relationship(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn batch_upsert_new_entities_with_rel_of_property_returns_201() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:004_01_03a");
    let id2 = unique_id("urn:ngsi-ld:Building:004_01_03b");

    let e1 = building_relationship_of_property(&id1);
    let e2 = building_relationship_of_property(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn batch_upsert_mixed_new_and_existing_returns_201() {
    clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:004_02_01_exist");
    let pre = building_simple_attributes(&existing_id);
    let (status, _, _) = create_entity(&c, &pre).await;
    assert_eq!(status, StatusCode::CREATED);

    let new_id = unique_id("urn:ngsi-ld:Building:004_02_01_new");
    let e1 = building_simple_attributes(&existing_id);
    let e2 = building_simple_attributes(&new_id);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED, "should return 201 for new entities");

    let (status, _, body) = get_entity(&c, &existing_id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");

    let (status, _, _) = get_entity(&c, &new_id, "", false).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn batch_upsert_mixed_new_and_existing_retrieval_returns_200() {
    clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:004_02_02_exist");
    let pre = building_minimal(&existing_id);
    let (status, _, _) = create_entity(&c, &pre).await;
    assert_eq!(status, StatusCode::CREATED);

    let new_id = unique_id("urn:ngsi-ld:Building:004_02_02_new");
    let e1 = building_simple_attributes(&existing_id);
    let e2 = building_simple_attributes(&new_id);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &existing_id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["subCategory"]["value"], "tourism");

    let (status, _, body) = get_entity(&c, &new_id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_upsert_mixed_with_relationships_returns_201() {
    clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:004_02_03_exist");
    let pre = building_minimal(&existing_id);
    let (status, _, _) = create_entity(&c, &pre).await;
    assert_eq!(status, StatusCode::CREATED);

    let new_id = unique_id("urn:ngsi-ld:Building:004_02_03_new");
    let e1 = building_relationship(&existing_id);
    let e2 = building_relationship(&new_id);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &existing_id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locatedAt"]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn batch_upsert_existing_replace_properties_returns_204() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:004_03_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:004_03_01b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        create_entity(&c, &entity).await;
    }

    let e1 = building_minimal(&id1);
    let e2 = building_minimal(&id2);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "upsert of all existing should return 204");

    let (status, _, body) = get_entity(&c, &id1, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_none(), "old attribute should be gone after replace");
    assert!(body.get("subCategory").is_none(), "old attribute should be gone after replace");
}

#[tokio::test]
#[serial]
async fn batch_upsert_existing_replace_relationships_returns_204() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:004_03_02");
    let entity = building_relationship(&id);
    create_entity(&c, &entity).await;

    let e1 = building_relationship_of_property(&id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn batch_upsert_existing_replace_rel_of_property_returns_204() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:004_03_03");
    let entity = building_relationship_of_property(&id);
    create_entity(&c, &entity).await;

    let e1 = building_simple_attributes(&id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_upsert_existing_replace_scope_returns_204() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:004_03_04");
    let entity = building_with_one_scope(&id);
    create_entity(&c, &entity).await;

    let e1 = building_minimal(&id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("scope").is_none(), "scope should be gone after replace");
}

#[tokio::test]
#[serial]
async fn batch_upsert_existing_replace_types_returns_204() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:004_03_05");
    let entity = building_minimal(&id);
    create_entity(&c, &entity).await;

    let e1 = building_with_two_types(&id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial]
async fn batch_upsert_with_update_option_returns_201() {
    clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:004_04_01_exist");
    let entity = building_simple_attributes(&existing_id);
    create_entity(&c, &entity).await;

    let new_id = unique_id("urn:ngsi-ld:Building:004_04_01_new");
    let e1 = building_relationship(&existing_id);
    let e2 = building_minimal(&new_id);
    let entities = json!([e1, e2]);

    let (status, _) = batch_upsert(&c, &entities, true).await;
    assert_eq!(status, StatusCode::CREATED, "should return 201 for new entity");

    let (status, _, body) = get_entity(&c, &existing_id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subCategory"]["value"], "tourism", "subCategory should be preserved (merge, not replace)");
    assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
}

#[tokio::test]
#[serial]
async fn batch_upsert_with_update_option_retrieval_returns_204() {
    clean_database().await;
    let c = client();

    let existing_id = unique_id("urn:ngsi-ld:Building:004_04_02_exist");
    let entity = building_simple_attributes(&existing_id);
    create_entity(&c, &entity).await;

    let e1 = building_relationship_of_property(&existing_id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, true).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &existing_id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn batch_upsert_with_update_option_preserves_scope_returns_204() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:004_04_03");
    let entity = building_with_one_scope(&id);
    create_entity(&c, &entity).await;

    let e1 = building_simple_attributes(&id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, true).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity(&c, &id, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("scope").is_some(), "scope should be preserved after merge");
    assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn batch_upsert_with_invalid_entity_returns_207() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:004_05_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:004_05_01b");
    let id3 = unique_id("urn:ngsi-ld:Building:004_05_01c");

    let e1 = building_minimal(&id1);
    let e2 = building_minimal(&id2);
    let entities = json!([
        e1,
        e2,
        { "id": &id3, "@context": TEST_SUITE_CONTEXT }
    ]);

    let (status, body) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::MULTI_STATUS);

    let success = body["success"].as_array().expect("should have success");
    let errors = body["errors"].as_array().expect("should have errors");
    assert_eq!(success.len(), 2);
    assert_eq!(errors.len(), 1);
}

#[tokio::test]
async fn batch_upsert_invalid_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(
        &c,
        Method::POST,
        "/entityOperations/upsert",
        b"[{\"id\":\"urn:ngsi-ld:Building:x\",,}]",
        Some("application/ld+json"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_upsert_empty_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/upsert", b"[]", Some("application/ld+json"), false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn batch_upsert_duplicate_ids_returns_201() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Building:004_07_01a");
    let id2 = unique_id("urn:ngsi-ld:Building:004_07_01b");

    let e1 = building_minimal(&id1);
    let e2 = building_minimal(&id2);
    let e3 = building_simple_attributes(&id1);
    let entities = json!([e1, e2, e3]);

    let (status, body) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::CREATED);

    let uris = body.as_array().expect("should be array of URIs");
    assert!(uris.len() <= 2, "should deduplicate IDs");
}

#[tokio::test]
#[serial]
async fn batch_upsert_with_new_types_returns_204() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:004_08_01");
    let entity = building_minimal(&id);
    create_entity(&c, &entity).await;

    let e1 = building_minimal_with_new_types(&id);
    let entities = json!([e1]);

    let (status, _) = batch_upsert(&c, &entities, false).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
