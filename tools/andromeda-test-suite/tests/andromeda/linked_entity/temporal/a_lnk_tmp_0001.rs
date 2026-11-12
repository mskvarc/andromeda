//! A temporal q hop follows every object of a relationship.

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
    http::operation::{NormalizedTemporal, QueryTemporalEvolution},
    model::{Property, Relationship, evolution::TemporalEntity},
    query::{Join, builders::SelectorTypeBuilder, q, temporal_query},
};

/// Pins: a temporal q hop over a Relationship whose history instance holds several objects selects the root when
/// the entity any one object names satisfies the term, the last object included, and not when none does, each
/// linked entity judged on its instances valid within the window of the query, as the join renders it
/// Clause: 4.9, 4.5.3, 4.5.23, 5.7.4.4, 4.11
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension admits the hop under the
/// Core rules over the window of the request, where clause 4.9 makes the target object "any object" of the
/// Relationship. The extension has an off switch, so this file holds only while the switch is on and must be run
/// with it on
#[gtest]
#[tokio::test]
async fn a_lnk_tmp_0001_a_temporal_q_hop_follows_every_object_of_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let part_type = namespace.entity_type("Part")?;
    let machine_type = namespace.entity_type("Machine")?;
    let part_a = namespace.urn("Part", "a")?;
    let part_b = namespace.urn("Part", "b")?;
    let part_c = namespace.urn("Part", "c")?;
    for (id, name) in [(&part_a, "A"), (&part_b, "B"), (&part_c, "C")] {
        let part = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(part_type.clone())
            .property("name", Property::builder(name).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&part).send().await?.created()?;
    }
    let machine_1 = namespace.urn("Machine", 1)?;
    let machine_2 = namespace.urn("Machine", 2)?;
    for (id, parts) in [
        (&machine_1, vec![part_a.clone(), part_b.clone(), part_c.clone()]),
        (&machine_2, vec![part_a.clone(), part_b.clone()]),
    ] {
        let machine = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(machine_type.clone())
            .relationship("hasPart", Relationship::builder(parts).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&machine).send().await?.created()?;
    }
    let temporal_q = temporal_query::after("2020-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&machine_type).build()?;
    for (name, machines) in [("C", 1), ("A", 2)] {
        let hop = q::attr("hasPart").linked("name").eq(name).build()?;
        let query = QueryTemporalEvolution::builder()
            .temporal_q(&temporal_q)
            .entity_type(&selector)
            .q(&hop)
            .format(NormalizedTemporal)
            .build();
        // The temporal query writes no `join`, so the parameters follow the query it writes. Each machine nests its
        // linked parts, which no model reads, so the answer is read as received.
        let answer = send_joined(&broker, &query, Join::Inline, 1).await?;
        expect_that!(answer.status(), eq(StatusCode::OK));
        expect_that!(answer.body(), json_count("$", machines));
        if machines == 1 {
            expect_that!(answer.body(), json_equals("$[0].id", machine_1.to_string()));
        }
    }
    Ok(())
}
