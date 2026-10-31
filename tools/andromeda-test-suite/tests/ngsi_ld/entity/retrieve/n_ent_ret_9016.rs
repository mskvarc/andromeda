//! Each format renders its own object list form.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals, json_exists},
    broker,
    namespace::Namespace,
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
    model::{Lossless, NgsiLdMediaType},
};

/// Pins: Retrieve Entity of a `ListRelationship` answers `objectList` as Relationship Objects holding only object
/// when normalized, as bare URIs when concise, and the attribute as the array of URIs when `keyValues`, never with
/// object or `previousObject`
/// Clause: 4.5.22.2, Table 5.2.37-1, 4.5.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9016_each_format_renders_its_own_object_list_form() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_per_format_id = namespace.urn("Building", "per-format")?;
    let building_type = namespace.entity_type("Building")?;
    let room = format!("urn:ngsi-ld:Room:{namespace}-1");
    let room_2 = format!("urn:ngsi-ld:Room:{namespace}-2");
    // The model writes an `objectList` item as an object, so the entity with plain URI items is sent as written.
    let request = json_request_to(
        &broker,
        Method::POST,
        "entities",
        &format!(
            "{{\n    \"id\": \"{building_per_format_id}\",\n    \"type\": \"{building_type}\",\n    \"hasRooms\": {{\n        \"type\": \"ListRelationship\",\n        \"objectList\": [\"{room}\", \"{room_2}\"]\n    }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let in_options = RepresentationSpelling::default().in_options();
    // Each representation is read as received: the test pins how every form spells `objectList`, which the model
    // reads into one shape.
    let normalized = json
        .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(&building_per_format_id).format(Lossless::Normalized).build())?)
        .await?;
    let body = normalized.body();
    expect_that!(body, json_equals("$.hasRooms.type", "ListRelationship"));
    expect_that!(body, json_count("$.hasRooms.objectList", 2));
    expect_that!(body, json_equals("$.hasRooms.objectList[0].object", room.as_str()));
    expect_that!(body, json_exists("$[?count($.hasRooms.objectList[0].*)==1]"));
    expect_that!(body, json_equals("$.hasRooms.objectList[1].object", room_2.as_str()));
    expect_that!(body, json_exists("$[?count($.hasRooms.objectList[1].*)==1]"));
    expect_that!(body, json_absent("$.hasRooms.object"));
    expect_that!(body, json_absent("$.hasRooms.previousObject"));
    let concise = json
        .send_unchecked(
            json.unchecked(
                &RetrieveEntity::builder()
                    .id(&building_per_format_id)
                    .format(Lossless::Concise)
                    .spelling(in_options)
                    .build(),
            )?,
        )
        .await?;
    let body = concise.body();
    expect_that!(body, json_count("$.hasRooms.objectList", 2));
    expect_that!(body, json_equals("$.hasRooms.objectList[0]", room.as_str()));
    expect_that!(body, json_equals("$.hasRooms.objectList[1]", room_2.as_str()));
    expect_that!(body, json_absent("$.hasRooms.object"));
    expect_that!(body, json_absent("$.hasRooms.previousObject"));
    let key_values = json
        .send_unchecked(
            json.unchecked(
                &RetrieveEntity::builder()
                    .id(&building_per_format_id)
                    .format(Simplified)
                    .spelling(in_options.key_values())
                    .build(),
            )?,
        )
        .await?;
    let body = key_values.body();
    expect_that!(body, json_count("$.hasRooms", 2));
    expect_that!(body, json_equals("$.hasRooms[0]", room.as_str()));
    expect_that!(body, json_equals("$.hasRooms[1]", room_2.as_str()));
    Ok(())
}
