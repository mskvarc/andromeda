//! A temporal q hop reads the linked entity over the window not its current state.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    linked_temporal_fixtures::send_joined,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{NormalizedTemporal, QueryTemporalEvolution},
    model::{Entity, Property, Relationship, evolution::TemporalEntity},
    query::{Join, builders::SelectorTypeBuilder, q, temporal_query},
};

/// Pins: a temporal q hop judges the linked entity on its instances valid within the window of the query, as the
/// join renders it, never on its current state: a Part named Z now whose history names it A at 12:30 is reached by
/// name=="A" over 12:00-13:00 and not by name=="Z", and a Part named A only at 13:30, after the window, is not
/// reached
/// Clause: 4.9, 4.5.23, 4.11, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension admits the hop under the
/// Core rules over the window of the request, rendering every Linked Entity over that window. The extension has an
/// off switch, so this file holds only while the switch is on and must be run with it on
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0018_a_temporal_q_hop_reads_the_linked_entity_over_the_window_not_its_current_state() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let part_type = namespace.entity_type("Part")?;
    let machine_type = namespace.entity_type("Machine")?;
    let part_a = namespace.urn("Part", "a")?;
    let part_b = namespace.urn("Part", "b")?;
    let current = Entity::builder()
        .id(part_a.clone())
        .entity_type(part_type.clone())
        .property("name", "Z")
        .build()?;
    broker.entities().create(&current).send().await?.done()?;
    let history = TemporalEntity::builder()
        .id(part_a.clone())
        .entity_type(part_type.clone())
        .property("name", Property::builder("A").observed_at("2020-08-01T12:30:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&part_a, &history).send().await?;
    let after_the_window = TemporalEntity::builder()
        .id(part_b.clone())
        .entity_type(part_type.clone())
        .property("name", Property::builder("A").observed_at("2020-08-01T13:30:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&after_the_window).send().await?.created()?;
    let machine_1 = namespace.urn("Machine", 1)?;
    let machine_2 = namespace.urn("Machine", 2)?;
    for (id, part) in [(&machine_1, &part_a), (&machine_2, &part_b)] {
        let machine = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(machine_type.clone())
            .relationship("hasPart", Relationship::builder(part.clone()).observed_at("2020-08-01T12:10:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&machine).send().await?.created()?;
    }
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&machine_type).build()?;
    for (name, hits) in [("A", 1), ("Z", 0)] {
        let hop = q::attr("hasPart").linked("name").eq(name).build()?;
        let query = QueryTemporalEvolution::builder()
            .temporal_q(&window)
            .entity_type(&selector)
            .q(&hop)
            .format(NormalizedTemporal)
            .build();
        // The temporal query writes no `join`, so the parameters follow the query it writes. A machine nests its linked
        // part, which no model reads, so the answer is read as received.
        let answer = send_joined(&broker, &query, Join::Inline, 1).await?;
        expect_that!(answer.status(), eq(StatusCode::OK));
        expect_that!(answer.body(), json_count("$", hits));
        if hits == 1 {
            expect_that!(answer.body(), json_equals("$[0].id", machine_1.to_string()));
        }
    }
    Ok(())
}
