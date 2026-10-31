//! A malformed type selection on a write is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Delete Entity, Replace Entity, Append, Update, Partial Attribute update, Delete Attribute and Replace
/// Attribute with type ending in a bare | each answer 400 `BadRequestData`, and the Vehicle reads back as before
/// Clause: 4.17, 5.5.2
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9008_a_malformed_type_selection_on_a_write_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // A type selection ending in a bare `|` cannot be built, so each write is sent as written.
    let vehicle_malformed_delete_entity_id = namespace.urn("Vehicle", "malformed-delete-entity")?;
    let vehicle_malformed_delete_entity = Entity::builder()
        .id(vehicle_malformed_delete_entity_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_delete_entity).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_delete_entity_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = request_to(
        &broker,
        Method::DELETE,
        &format!("entities/{vehicle_malformed_delete_entity_id}?type=Vehicle_{namespace}%7C"),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_delete_entity_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_malformed_replace_entity_id = namespace.urn("Vehicle", "malformed-replace-entity")?;
    let vehicle_malformed_replace_entity = Entity::builder()
        .id(vehicle_malformed_replace_entity_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_replace_entity).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_replace_entity_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = json_request_to(
        &broker,
        Method::PUT,
        &format!("entities/{vehicle_malformed_replace_entity_id}?type=Vehicle_{namespace}%7C"),
        &format!("{{\n    \"type\": \"Vehicle_{namespace}\",\n    \"speed\": {{\n        \"type\": \"Property\",\n        \"value\": 1\n    }}\n}}"),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_replace_entity_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_malformed_append_attributes_id = namespace.urn("Vehicle", "malformed-append-attributes")?;
    let vehicle_malformed_append_attributes = Entity::builder()
        .id(vehicle_malformed_append_attributes_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_append_attributes).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_append_attributes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = json_request_to(
        &broker,
        Method::POST,
        &format!("entities/{vehicle_malformed_append_attributes_id}/attrs?type=Vehicle_{namespace}%7C"),
        "{\n    \"color\": {\n        \"type\": \"Property\",\n        \"value\": \"red\"\n    }\n}",
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_append_attributes_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_malformed_update_attributes_id = namespace.urn("Vehicle", "malformed-update-attributes")?;
    let vehicle_malformed_update_attributes = Entity::builder()
        .id(vehicle_malformed_update_attributes_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_update_attributes).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_update_attributes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_malformed_update_attributes_id}/attrs?type=Vehicle_{namespace}%7C"),
        "{\n    \"speed\": {\n        \"type\": \"Property\",\n        \"value\": 70\n    }\n}",
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_update_attributes_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_malformed_partial_attribute_update_id = namespace.urn("Vehicle", "malformed-partial-attribute-update")?;
    let vehicle_malformed_partial_attribute_update = Entity::builder()
        .id(vehicle_malformed_partial_attribute_update_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_partial_attribute_update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_partial_attribute_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_malformed_partial_attribute_update_id}/attrs/speed?type=Vehicle_{namespace}%7C"),
        "{\n    \"value\": 71\n}",
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_partial_attribute_update_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_malformed_delete_attribute_id = namespace.urn("Vehicle", "malformed-delete-attribute")?;
    let vehicle_malformed_delete_attribute = Entity::builder()
        .id(vehicle_malformed_delete_attribute_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_delete_attribute).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_delete_attribute_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = request_to(
        &broker,
        Method::DELETE,
        &format!("entities/{vehicle_malformed_delete_attribute_id}/attrs/speed?type=Vehicle_{namespace}%7C"),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_delete_attribute_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_malformed_replace_attribute_id = namespace.urn("Vehicle", "malformed-replace-attribute")?;
    let vehicle_malformed_replace_attribute = Entity::builder()
        .id(vehicle_malformed_replace_attribute_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_malformed_replace_attribute).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_replace_attribute_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let request_2 = json_request_to(
        &broker,
        Method::PUT,
        &format!("entities/{vehicle_malformed_replace_attribute_id}/attrs/speed?type=Vehicle_{namespace}%7C"),
        "{\n    \"type\": \"Property\",\n    \"value\": 72\n}",
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_malformed_replace_attribute_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    Ok(())
}
