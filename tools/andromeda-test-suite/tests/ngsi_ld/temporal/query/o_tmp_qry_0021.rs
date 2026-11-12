//! The last pointer leads to the final page.

use andromeda_test_suite::{broker, cursor_walk, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{NormalizedTemporal, QueryTemporalEvolution},
    },
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with limit 2 and an empty cursor over five members offers a last pointer
/// leading to a page of two members holding the greatest identifier, with no next pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a query
/// in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last pointers
/// beside next and prev
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0021_the_last_pointer_leads_to_the_final_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0004_id = namespace.urn("Sensor", "0004")?;
    let evolutions = (0..=4)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("Sensor", format!("{n:04}"))?)
                .entity_type(sensor_type.clone())
                .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let json_ld = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let window = temporal_query::after("1970-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&sensor_type).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .locality(Locality::Local)
        .limit(2)
        .format(NormalizedTemporal)
        .build();
    let first = cursor_walk::first_page(&json_ld, &query).await?.page;
    let last = json_ld.send(&first.last_page().or_fail()?).await?;
    expect_that!(last.items(), len(eq(2)));
    expect_that!(last.items(), contains(has_id(&sensor_0004_id)));
    expect_that!(last.next(), none());
    Ok(())
}
