//! The page links of a posted temporal query read the held query by its key.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    page_link::{carries, carries_parameter},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities through POST `/temporal/entityOperations/query` with `type`, `q`, a
/// `temporalQ`, limit 2 and count answers a next Link that is a GET on `/temporal/entities` carrying `postedQuery`,
/// limit 2 and count and none of `type`, `q`, `timerel`, `timeAt`, `pageQuery` and `pageContext`; the prev Link of the
/// next page is a GET on `/temporal/entities` naming the same `postedQuery`
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9095_the_page_links_of_a_posted_temporal_query_read_the_held_query_by_its_key() -> Result<()> {
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
    let first = json
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(0))
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    let next = first.next().or_fail()?;
    expect_that!(next.path(), ends_with("/ngsi-ld/v1/temporal/entities"));
    expect_that!(next, carries_parameter("postedQuery"));
    expect_that!(next, carries("limit", "2"));
    expect_that!(next, carries("count", "true"));
    for member in ["type", "q", "timerel", "timeAt", "pageQuery", "pageContext"] {
        expect_that!(next, not(carries_parameter(member)));
    }
    let key = next
        .query_pairs()
        .find(|(name, _)| name == "postedQuery")
        .map(|(_, value)| value.into_owned())
        .or_fail()?;
    let second = json.send(&first.next_page().or_fail()?).await?;
    expect_that!(second.items(), len(eq(1)));
    let prev = second.prev().or_fail()?;
    expect_that!(prev.path(), ends_with("/ngsi-ld/v1/temporal/entities"));
    expect_that!(prev, carries("postedQuery", &key));
    Ok(())
}
