//! A percent encoded relationship object is recorded as written.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, relationship},
    body_json::json_equals,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    simplified_pairs::{object_list_pairs, object_pairs},
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RetrieveEntity, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{AttributeKind, EntityId, ListRelationship, Lossless, NgsiLdMediaType, OneOrMany, Relationship, evolution::TemporalEntity},
};

/// Pins: A Create Temporal Evolution whose Relationship object and `ListRelationship` `objectList` percent-encode the colons
/// after the scheme is retrieved with every target exactly as written, in the normalized and the temporalValues
/// representations, and current state holds the same spelling
/// Clause: 5.6.11.4, 5.7.3.4, 4.5.3.2, 4.5.9, 4.5.22.2; IETF RFC 3986 sections 2.2 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9026_a_percent_encoded_relationship_object_is_recorded_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let room_encoded_id = namespace.urn("Room", "encoded")?;
    let room_type = namespace.entity_type("Room")?;
    let building = format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1");
    let room = format!("urn:ngsi-ld%3ARoom%3A{namespace}-2");
    let building_id = building.parse::<EntityId>()?;
    let room_id = room.parse::<EntityId>()?;
    let room_encoded_evolution = TemporalEntity::builder()
        .id(room_encoded_id.clone())
        .entity_type(room_type)
        .relationship(
            "locatedIn",
            Relationship::builder(building_id.clone()).observed_at("2024-06-01T00:00:00Z".parse()?),
        )
        .list_relationship(
            "adjacent",
            ListRelationship::builder(vec![room_id.clone()]).observed_at("2024-06-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&room_encoded_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client.temporal().retrieve(&room_encoded_id).send().await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("locatedIn", nth(0, relationship().object(building_id.clone()))));
    expect_that!(
        evolution,
        has_evolution(
            "adjacent",
            nth(0, attribute_of(AttributeKind::ListRelationship).list_object(0, room_id.clone()))
        )
    );
    let retrieved_2 = client.temporal().retrieve_values(&room_encoded_id).send().await?;
    let values = retrieved_2.body();
    expect_that!(
        object_pairs(values, "locatedIn").and_then(|pairs| pairs.first()).map(|pair| &pair.value),
        some(eq(&OneOrMany::one(building_id.clone())))
    );
    expect_that!(
        object_list_pairs(values, "adjacent").and_then(|pairs| pairs.first()).map(|pair| &pair.value),
        some(eq(&vec![room_id]))
    );
    let retrieved_3 = client.entities().retrieve(&room_encoded_id).send().await?;
    expect_that!(retrieved_3.body(), has_attribute("locatedIn", relationship().object(building_id)));
    // The typed identifier cannot tell two spellings of one URI apart, so the answers are also read as received.
    let selection = TemporalSelection::Unbounded {
        time_property: None,
        last_n: None,
        aggregation_methods: None,
        aggregation_period: None,
    };
    let temporal = RetrieveTemporalEvolution::builder()
        .id(&room_encoded_id)
        .selection(selection)
        .format(Lossless::Normalized)
        .build();
    let answer = send_versioned(&client, &temporal, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(&answer.body, json_equals("$.locatedIn[0].object", building.clone()));
    expect_that!(&answer.body, json_equals("$.adjacent[0].objectList[0].object", room.clone()));
    let current = RetrieveEntity::builder().id(&room_encoded_id).format(Lossless::Normalized).build();
    let answer_2 = send_versioned(&client, &current, None).await?;
    expect_that!(&answer_2.body, json_equals("$.locatedIn.object", building));
    Ok(())
}
