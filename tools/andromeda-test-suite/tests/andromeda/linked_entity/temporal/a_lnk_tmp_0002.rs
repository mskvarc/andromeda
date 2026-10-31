//! A temporal q hop follows every object of a list relationship.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    linked_temporal_fixtures::send_joined,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::QueryTemporalEvolution,
    model::{ListRelationship, Lossless, Property, evolution::TemporalEntity},
    query::{Join, builders::SelectorTypeBuilder, q, temporal_query},
};

/// Pins: a temporal q hop over a `ListRelationship` selects the root when the entity any one object of its
/// `objectList` names satisfies the term, the last object included, and not when none does, each linked entity
/// judged on its instances valid within the window of the query, as the join renders it
/// Clause: 4.9, 4.5.22, 4.5.23, 5.7.4.4, 4.11
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension admits the hop under the
/// Core rules over the window of the request, where clause 4.9 makes the target object of a `ListRelationship` its
/// array of objects. The extension has an off switch, so this file holds only while the switch is on and must be
/// run with it on
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0002_a_temporal_q_hop_follows_every_object_of_a_list_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let building_type = namespace.entity_type("Building")?;
    let device_a = namespace.urn("Device", "a")?;
    let device_b = namespace.urn("Device", "b")?;
    let device_c = namespace.urn("Device", "c")?;
    for (id, name) in [(&device_a, "DevA"), (&device_b, "DevB"), (&device_c, "DevC")] {
        let device = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(device_type.clone())
            .property("name", Property::builder(name).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&device).send().await?.created()?;
    }
    let building_1 = namespace.urn("Building", 1)?;
    let building_2 = namespace.urn("Building", 2)?;
    for (id, devices) in [
        (&building_1, vec![device_a.clone(), device_b.clone(), device_c.clone()]),
        (&building_2, vec![device_a.clone(), device_b.clone()]),
    ] {
        let building = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .list_relationship("hasDevices", ListRelationship::builder(devices).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&building).send().await?.created()?;
    }
    let temporal_q = temporal_query::after("2020-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    for (name, buildings) in [("DevC", 1), ("DevA", 2)] {
        let hop = q::attr("hasDevices").linked("name").eq(name).build()?;
        let query = QueryTemporalEvolution::builder()
            .temporal_q(&temporal_q)
            .entity_type(&selector)
            .q(&hop)
            .format(Lossless::Normalized)
            .build();
        // The temporal query writes no `join`, so the parameters follow the query it writes. Each building nests its
        // linked devices, which no model reads, so the answer is read as received.
        let answer = send_joined(&broker, &query, Join::Inline, 1).await?;
        expect_that!(answer.status(), eq(StatusCode::OK));
        expect_that!(answer.body(), json_count("$", buildings));
        if buildings == 1 {
            expect_that!(answer.body(), json_equals("$[0].id", building_1.to_string()));
        }
    }
    Ok(())
}
