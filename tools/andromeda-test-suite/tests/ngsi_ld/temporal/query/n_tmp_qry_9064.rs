//! A type query pages its evolutions in identifier order past another type.

use andromeda_test_suite::{
    broker,
    identity_matcher::{has_id, ids_in_order},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query, type_selection},
};

/// Pins: Query Temporal Evolution of Entities by type over 320 evolutions of which only the last 20 in identifier
/// order carry the queried type pages exactly those 20 in identifier order, ten at a time, with a total of 20, by
/// GET and by POST query; an evolution of the other type that a second Create or Update Temporal Evolution gives
/// the queried type joins the answer at its own place, and a selection of both types lists every evolution once
/// Clause: 5.7.4.4, 5.5.9, 5.6.11.4, 4.17
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9064_a_type_query_pages_its_evolutions_in_identifier_order_past_another_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let type_walk_0001_id = namespace.urn("TypeWalk", "0001")?;
    let other_type = namespace.entity_type("Other")?;
    let type_walk_0301_id = namespace.urn("TypeWalk", "0301")?;
    let walked_type = namespace.entity_type("Walked")?;
    let type_walk_0302_id = namespace.urn("TypeWalk", "0302")?;
    let type_walk_0320_id = namespace.urn("TypeWalk", "0320")?;
    let type_walk_0002_id = namespace.urn("TypeWalk", "0002")?;
    let type_walk_0003_id = namespace.urn("TypeWalk", "0003")?;
    let type_walk_0300_id = namespace.urn("TypeWalk", "0300")?;
    let evolutions = (1..=300)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("TypeWalk", format!("{n:04}"))?)
                .entity_type(other_type.clone())
                .property("level", Property::builder(n).observed_at("2026-03-01T10:00:00Z".parse()?))
                .build()?)
        })
        .chain((301..=320).map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("TypeWalk", format!("{n:04}"))?)
                .entity_type(walked_type.clone())
                .property("level", Property::builder(n).observed_at("2026-03-01T10:00:00Z".parse()?))
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(10)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(20)));
    let expected_ids = (301..=310)
        .map(|n| namespace.urn("TypeWalk", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(10)
        .offset(10)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(20)));
    let expected_ids_2 = (311..=320)
        .map(|n| namespace.urn("TypeWalk", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(10)
        .offset(20)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(20)));
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(10)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(20)));
    let expected_ids_3 = (301..=310)
        .map(|n| namespace.urn("TypeWalk", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_4.items(), ids_in_order(&expected_ids_3));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(10)
        .offset(10)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(20)));
    let expected_ids_4 = (311..=320)
        .map(|n| namespace.urn("TypeWalk", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_5.items(), ids_in_order(&expected_ids_4));
    // clause 5.6.11.4 adds the type names an upsert carries to the evolution's own
    let type_walk_0001_evolution = TemporalEntity::builder()
        .id(type_walk_0001_id.clone())
        .entity_type(walked_type.clone())
        .property("level", Property::builder(1001).observed_at("2026-03-01T11:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&type_walk_0001_evolution).send().await?.updated()?;
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(3)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_6.total(), some(eq(21)));
    expect_that!(page_6.items(), len(eq(3)));
    let item = page_6.items().first().or_fail()?;
    expect_that!(item.entity_type.as_slice(), contains(eq(&other_type)));
    expect_that!(item.entity_type.as_slice(), contains(eq(&walked_type)));
    expect_that!(page_6.items(), nth(0, has_id(&type_walk_0001_id)));
    expect_that!(page_6.items(), nth(1, has_id(&type_walk_0301_id)));
    expect_that!(page_6.items(), nth(2, has_id(&type_walk_0302_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&walked_type)
        .locality(Locality::Local)
        .limit(10)
        .offset(20)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_7.total(), some(eq(21)));
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&type_walk_0320_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&other_type)
        .locality(Locality::Local)
        .limit(3)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_8.total(), some(eq(300)));
    expect_that!(page_8.items(), len(eq(3)));
    expect_that!(page_8.items(), nth(0, has_id(&type_walk_0001_id)));
    expect_that!(page_8.items(), nth(1, has_id(&type_walk_0002_id)));
    expect_that!(page_8.items(), nth(2, has_id(&type_walk_0003_id)));
    // an evolution carrying both types is listed once
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type(&other_type).or(&walked_type))
        .locality(Locality::Local)
        .limit(2)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_9.total(), some(eq(320)));
    expect_that!(page_9.items(), len(eq(2)));
    expect_that!(page_9.items(), nth(0, has_id(&type_walk_0001_id)));
    expect_that!(page_9.items(), nth(1, has_id(&type_walk_0002_id)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type(&other_type).or(&walked_type))
        .locality(Locality::Local)
        .limit(3)
        .offset(299)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_10.total(), some(eq(320)));
    expect_that!(page_10.items(), len(eq(3)));
    expect_that!(page_10.items(), nth(0, has_id(&type_walk_0300_id)));
    expect_that!(page_10.items(), nth(1, has_id(&type_walk_0301_id)));
    expect_that!(page_10.items(), nth(2, has_id(&type_walk_0302_id)));
    Ok(())
}
