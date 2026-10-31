//! Both written object list forms read back alike.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_exists},
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity, Simplified},
    },
    model::{Entity, EntityId, ListRelationship, Lossless, NgsiLdMediaType},
};

/// Pins: A `ListRelationship` written with a concise `objectList` and one written with a normalized `objectList`
/// each read back as Relationship Objects when normalized, bare URIs when concise, and the array of URIs when
/// `keyValues`
/// Clause: 4.5.22.2, 4.5.22.3, Table 5.2.37-1, 4.5.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9017_both_written_object_list_forms_read_back_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_written_concise_id = namespace.urn("Building", "written-concise")?;
    let building_written_normalized_id = namespace.urn("Building", "written-normalized")?;
    let building_type = namespace.entity_type("Building")?;
    let room = format!("urn:ngsi-ld:Room:{namespace}-1");
    let room_2 = format!("urn:ngsi-ld:Room:{namespace}-2");
    // The model writes an `objectList` item as an object, so the entity with plain URI items is sent as written.
    let request = json_request_to(
        &broker,
        Method::POST,
        "entities",
        &format!(
            "{{\n    \"id\": \"{building_written_concise_id}\",\n    \"type\": \"{building_type}\",\n    \"hasRooms\": {{\n        \"type\": \"ListRelationship\",\n        \"objectList\": [\"{room}\", \"{room_2}\"]\n    }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let building_written_normalized = Entity::builder()
        .id(building_written_normalized_id.clone())
        .entity_type(building_type)
        .list_relationship(
            "hasRooms",
            ListRelationship::builder(vec![room.parse::<EntityId>()?, room_2.parse::<EntityId>()?]),
        )
        .build()?;
    broker.entities().create(&building_written_normalized).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let in_options = RepresentationSpelling::default().in_options();
    // Each representation is read as received: the test pins how every form spells `objectList`, which the model
    // reads into one shape.
    for id in [&building_written_concise_id, &building_written_normalized_id] {
        let normalized = json
            .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(id).format(Lossless::Normalized).build())?)
            .await?;
        let body = normalized.body();
        expect_that!(body, json_count("$.hasRooms.objectList", 2));
        expect_that!(body, json_equals("$.hasRooms.objectList[0].object", room.as_str()));
        expect_that!(body, json_exists("$[?count($.hasRooms.objectList[0].*)==1]"));
        expect_that!(body, json_equals("$.hasRooms.objectList[1].object", room_2.as_str()));
        expect_that!(body, json_exists("$[?count($.hasRooms.objectList[1].*)==1]"));
        let concise = json
            .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(id).format(Lossless::Concise).spelling(in_options).build())?)
            .await?;
        let body = concise.body();
        expect_that!(body, json_count("$.hasRooms.objectList", 2));
        expect_that!(body, json_equals("$.hasRooms.objectList[0]", room.as_str()));
        expect_that!(body, json_equals("$.hasRooms.objectList[1]", room_2.as_str()));
        let key_values = json
            .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(id).format(Simplified).spelling(in_options.key_values()).build())?)
            .await?;
        let body = key_values.body();
        expect_that!(body, json_count("$.hasRooms", 2));
        expect_that!(body, json_equals("$.hasRooms[0]", room.as_str()));
        expect_that!(body, json_equals("$.hasRooms[1]", room_2.as_str()));
    }
    Ok(())
}
