//! A deleted list relationship reads back as a bare null.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    body_json::{json_absent, json_count, json_equals},
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InstanceSelection, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{AttributeKind, AttributeName, Entity, ListRelationship, Lossless, NgsiLdMediaType},
    query::temporal_query::{TemporalProperty, TemporalQuery},
};

/// Pins: Retrieve Temporal Evolution by deletedAt of an Entity whose `ListRelationship` was deleted answers the deletion
/// with objectList [`"urn:ngsi-ld:null"`], the Null bare and not a Relationship Object, and neither object nor
/// previousObject
/// Clause: 4.5.22.2, 4.5.0, 4.5.8
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9007_a_deleted_list_relationship_reads_back_as_a_bare_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_object_list_deleted_id = namespace.urn("Building", "object-list-deleted")?;
    let building_type = namespace.entity_type("Building")?;
    let building_object_list_deleted = Entity::builder()
        .id(building_object_list_deleted_id.clone())
        .entity_type(building_type)
        .list_relationship(
            "hasRooms",
            ListRelationship::builder(vec![namespace.urn("Room", 1)?, namespace.urn("Room", 2)?]),
        )
        .build()?;
    broker.entities().create(&building_object_list_deleted).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_object_list_deleted_id, &"hasRooms".parse::<AttributeName>()?)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    let window = TemporalQuery::after("2020-01-01T00:00:00Z".parse()?).with_time_property(TemporalProperty::DeletedAt);
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client
        .temporal()
        .retrieve(&building_object_list_deleted_id)
        .temporal_q(window.clone())
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "hasRooms",
            contains(all!(attribute_of(AttributeKind::ListRelationship).list_length(1), deleted_instance()))
        )
    );
    // The typed reader holds the Null as an object id, so the bare spelling is read as received.
    let operation = RetrieveTemporalEvolution::builder()
        .id(&building_object_list_deleted_id)
        .selection(TemporalSelection::Query(&window))
        .format(Lossless::Normalized)
        .build();
    let answer = send_versioned(&client, &operation, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    let body = &answer.body;
    expect_that!(body, json_count("$.hasRooms[?@.deletedAt].objectList", 1));
    expect_that!(body, json_equals("$.hasRooms[?@.deletedAt].objectList[0]", "urn:ngsi-ld:null"));
    expect_that!(body, json_absent("$.hasRooms[?@.deletedAt].object"));
    expect_that!(body, json_absent("$.hasRooms[?@.deletedAt].previousObject"));
    Ok(())
}
