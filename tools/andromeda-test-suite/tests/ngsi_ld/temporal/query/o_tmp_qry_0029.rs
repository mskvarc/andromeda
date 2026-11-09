//! An entity map created from a cursor page maps every matching entity.

use andromeda_test_suite::{broker, cursor_walk, followed_link::request_with, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Accept, Link, NgsiLdEntityMap, RelationType},
        operation::QueryTemporalEvolution,
    },
    model::{Input, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities started with an empty cursor and followed by its next pointer with
/// entityMap true added, over three entities, answers 201 with an `EntityMap` that maps all three entities
/// Clause: 5.5.14, 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: a next pointer is Andromeda's opaque cursor; a request built from it by adding entityMap is a use of that
/// form
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0029_an_entity_map_created_from_a_cursor_page_maps_every_matching_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_ids = (0..=2).map(|n| namespace.urn("Sensor", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    for id in &sensor_ids {
        let evolution: TemporalEntity<Input> = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let window = temporal_query::after("1970-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&sensor_type).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .locality(Locality::Local)
        .limit(1)
        .format(Lossless::Normalized)
        .build();
    let walk = cursor_walk::first_page(&broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)), &query).await?;
    expect_that!(walk.page.next(), some(anything()));
    let link = walk.head.typed::<Link>().into_valid().or_fail()?;
    let request =
        request_with(&broker, &link, &RelationType::Next, &[("entityMap", "true")])?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(request).await?;
    expect_that!(answer.status(), eq(StatusCode::CREATED));
    let map_id = ResponseHead::new(answer.headers().clone())
        .typed::<NgsiLdEntityMap>()
        .into_valid()
        .and_then(|header| header.id())
        .or_fail()?;
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(&map.entity_map, len(eq(3)));
    for id in &sensor_ids {
        expect_that!(map.entity_map.get(id), some(anything()));
    }
    Ok(())
}
