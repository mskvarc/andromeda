//! Empty periods take no room on a page.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: three instances ten days apart aggregated per PT1M over 30 days answer 200 with three periods, although
/// the window holds 43,200 of them
/// Clause: 4.5.19, 6.3.10
/// Recorded: a period holding no instance has no triple, so the page counts only the periods that hold one
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9021_empty_periods_take_no_room_on_a_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-11T00:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-21T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-31T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("PT1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.head.typed::<ContentRange>(), absent());
    let retrieved = &headed.outcome;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::TotalCount, len(eq(3))));
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                nth(0, period().start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T00:01:00Z".parse()?)),
                nth(1, period().start("2024-01-11T00:00:00Z".parse()?)),
                nth(2, period().value(3).start("2024-01-21T00:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
