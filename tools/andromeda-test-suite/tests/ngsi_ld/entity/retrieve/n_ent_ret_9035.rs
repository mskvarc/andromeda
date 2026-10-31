//! Percent encoded relationship targets read back as written in every representation.

use andromeda_test_suite::{body_json::json_equals, broker, namespace::Namespace, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity, Simplified},
    },
    model::{Lossless, NgsiLdMediaType},
};

/// Pins: An Entity whose Relationship object and `ListRelationship` `objectList` percent-encode the colons after
/// the scheme is retrieved with every target exactly as written in the normalized, concise and `keyValues`
/// representations
/// Clause: 4.5.3.2, 4.5.3.3, 4.5.4, 4.5.22.2, 4.5.22.3, 5.7.1; IETF RFC 3986 sections 2.2 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_ent_ret_9035_percent_encoded_relationship_targets_read_back_as_written_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let room_encoded_id = namespace.urn("Room", "encoded")?;
    // The model rewrites a target spelling, which this test keeps, so the entity is sent as written.
    let request = json_request_to(
        &broker,
        Method::POST,
        "entities",
        &format!(
            "{{\n    \"id\": \"{room_encoded_id}\",\n    \"type\": \"Room_{namespace}\",\n    \"locatedIn\": {{\n        \"type\": \"Relationship\",\n        \"object\": \"urn:ngsi-ld%3ABuilding%3A{namespace}-1\"\n    }},\n    \"adjacent\": {{\n        \"type\": \"ListRelationship\",\n        \"objectList\": [\"urn:ngsi-ld%3ARoom%3A{namespace}-2\", \"urn:ngsi-ld:Room:{namespace}-3\"]\n    }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let located_in = format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1");
    let adjacent = format!("urn:ngsi-ld%3ARoom%3A{namespace}-2");
    let adjacent_2 = format!("urn:ngsi-ld:Room:{namespace}-3");
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // Each representation is read as received, since the model rewrites the target spelling the test pins.
    let retrieved_2 = json
        .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(&room_encoded_id).format(Lossless::Normalized).build())?)
        .await?;
    let body_2 = retrieved_2.body();
    expect_that!(body_2, json_equals("$.locatedIn.object", located_in.as_str()));
    expect_that!(body_2, json_equals("$.adjacent.objectList[0].object", adjacent.as_str()));
    expect_that!(body_2, json_equals("$.adjacent.objectList[1].object", adjacent_2.as_str()));
    let retrieved_3 = json
        .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(&room_encoded_id).format(Lossless::Concise).build())?)
        .await?;
    let body_3 = retrieved_3.body();
    expect_that!(body_3, json_equals("$.locatedIn.object", located_in.as_str()));
    expect_that!(body_3, json_equals("$.adjacent.objectList[0]", adjacent.as_str()));
    let retrieved_4 = json
        .send_unchecked(
            json.unchecked(
                &RetrieveEntity::builder()
                    .id(&room_encoded_id)
                    .format(Simplified)
                    .spelling(RepresentationSpelling::default().key_values())
                    .build(),
            )?,
        )
        .await?;
    let body_4 = retrieved_4.body();
    expect_that!(body_4, json_equals("$.locatedIn", located_in.as_str()));
    expect_that!(body_4, json_equals("$.adjacent[0]", adjacent.as_str()));
    expect_that!(body_4, json_equals("$.adjacent[1]", adjacent_2.as_str()));
    Ok(())
}
