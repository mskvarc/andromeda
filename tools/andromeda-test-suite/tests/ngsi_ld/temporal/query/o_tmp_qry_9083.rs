//! An offset at or past the end answers an empty array with only prev.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with limit 2 over two entities and an offset of 2 or 3 answers 200
/// with an empty array, `NGSILD-Results-Count` 2 and a prev pointer alone
/// Clause: 5.5.9.2, 6.3.10, Table 6.3.10-2
/// Recorded: Table 6.3.10-2 makes the too-large offset a "should" and its rule sets the offset to the size of the result
/// set, which selects nothing; Andromeda answers 200 with an empty array
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9083_an_offset_at_or_past_the_end_answers_an_empty_array_with_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    for n in 0..2 {
        let sensor = TemporalEntity::builder()
            .id(namespace.urn("Sensor", format!("{n:04}"))?)
            .entity_type(sensor_type.clone())
            .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&sensor).send().await?.created()?;
    }
    for offset in [2, 3] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .temporal()
            .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
            .entity_type(&sensor_type)
            .locality(Locality::Local)
            .limit(2)
            .count(ResultsCount::Include)
            .offset(offset)
            .send()
            .await?;
        expect_that!(page.items(), is_empty());
        expect_that!(page.total(), some(eq(2)));
        expect_that!(page.prev(), some(anything()));
        expect_that!(page.next(), none());
        expect_that!(page.first(), none());
        expect_that!(page.last(), none());
    }
    Ok(())
}
