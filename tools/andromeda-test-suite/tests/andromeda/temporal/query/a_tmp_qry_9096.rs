//! The same temporal query posted twice points at the same page.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities through POST `/temporal/entityOperations/query` with the same body and
/// parameters twice answers the same next Link both times, and a body differing in `q` answers another
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9096_the_same_temporal_query_posted_twice_points_at_the_same_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for number in 1..=3 {
        let evolution = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(number).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let mut nexts = Vec::new();
    for threshold in [0, 0, 1] {
        let page = json
            .temporal()
            .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
            .method(QueryMethod::Post)
            .entity_type(&vehicle_type)
            .q(q::attr("speed").gt(threshold))
            .limit(1)
            .send()
            .await?;
        nexts.push(page.next().or_fail()?.clone());
    }
    expect_that!(&nexts[0], eq(&nexts[1]));
    expect_that!(&nexts[0], not(eq(&nexts[2])));
    Ok(())
}
