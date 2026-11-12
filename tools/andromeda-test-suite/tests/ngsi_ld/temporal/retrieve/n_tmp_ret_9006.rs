//! A temporal evolution keeps relationship objects.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    body_json::{json_absent, json_count, json_equals},
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    versioned_answer::send_versioned,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{AttributeKind, NgsiLdMediaType},
    query::temporal_query::{self, TemporalQuery},
};

/// Pins: Retrieve Temporal Evolution of a `ListRelationship` instance written with a concise objectList answers that
/// objectList as Relationship Objects holding only object, and neither object nor previousObject
/// Clause: 4.5.22.2, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9006_a_temporal_evolution_keeps_relationship_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_object_list_temporal_id = namespace.urn("Building", "object-list-temporal")?;
    let building_type = namespace.entity_type("Building")?;
    let room_1_id = namespace.urn("Room", 1)?;
    let room_2_id = namespace.urn("Room", 2)?;
    // The client writes an `objectList` as objects, and the point here is the string spelling a client may send.
    let body = format!(
        r#"{{"id": "{building_object_list_temporal_id}", "type": "{building_type}", "hasRooms": [{{"type": "ListRelationship", "objectList": ["{room_1_id}", "{room_2_id}"], "observedAt": "2022-08-09T18:25:02Z"}}]}}"#
    );
    let request = json_request_to(&broker, Method::POST, "temporal/entities", &body)?;
    let created = broker.send_unchecked(request).await?;
    expect_that!(created.status(), eq(StatusCode::CREATED));
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client
        .temporal()
        .retrieve(&building_object_list_temporal_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "hasRooms",
            all!(
                len(eq(1)),
                nth(
                    0,
                    attribute_of(AttributeKind::ListRelationship)
                        .list_length(2)
                        .list_object(0, room_1_id.clone())
                        .list_object(1, room_2_id.clone())
                )
            )
        )
    );
    // The typed reader holds the objects as a list of ids, so the members of each object are read as received.
    let window = TemporalQuery::after("2020-01-01T00:00:00Z".parse()?);
    let operation = RetrieveTemporalEvolution::builder()
        .id(&building_object_list_temporal_id)
        .selection(TemporalSelection::Query(&window))
        .format(NormalizedTemporal)
        .build();
    let answer = send_versioned(&client, &operation, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    let body = &answer.body;
    expect_that!(body, json_count("$.hasRooms[0].objectList", 2));
    expect_that!(body, json_equals("$.hasRooms[0].objectList[0].object", room_1_id.to_string()));
    expect_that!(body, json_count("$.hasRooms[0].objectList[0]", 1));
    expect_that!(body, json_equals("$.hasRooms[0].objectList[1].object", room_2_id.to_string()));
    expect_that!(body, json_count("$.hasRooms[0].objectList[1]", 1));
    expect_that!(body, json_absent("$.hasRooms[0].object"));
    expect_that!(body, json_absent("$.hasRooms[0].previousObject"));
    Ok(())
}
