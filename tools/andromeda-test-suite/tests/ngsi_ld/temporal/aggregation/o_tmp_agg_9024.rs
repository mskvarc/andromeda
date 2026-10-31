//! A month period pages on its calendar grid.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: 21 monthly instances on the 20th from 2024-01-20 aggregated per P1M between 2024-01-15 and 2026-01-15
/// answer 206 with 19 periods from the 15th to the 15th and Content-Range `DateTime`
/// 2024-01-15T00:00:00Z-2025-08-15T00:00:00Z/*; resumed at 2025-08-15 they answer 200 with the last two
/// Clause: 4.5.19, 4.5.19.1, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 periods per attribute; a
/// calendar page ends on its grid, and from the 15th a resumed page meets the same boundaries
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9024_a_month_period_pages_on_its_calendar_grid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(0).observed_at("2024-01-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-02-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-03-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-04-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-05-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-06-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-07-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-08-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2024-09-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2024-10-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(10).observed_at("2024-11-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2024-12-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(12).observed_at("2025-01-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(13).observed_at("2025-02-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(14).observed_at("2025-03-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(15).observed_at("2025-04-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(16).observed_at("2025-05-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(17).observed_at("2025-06-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(18).observed_at("2025-07-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(19).observed_at("2025-08-20T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2025-09-20T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-15T00:00:00Z".parse()?, "2026-01-15T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-15T00:00:00Z".parse()?,
            "2025-08-15T00:00:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let retrieved = &headed.outcome;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                len(eq(19)),
                nth(0, period().value(0).start("2024-01-15T00:00:00Z".parse()?).end("2024-02-15T00:00:00Z".parse()?)),
                nth(
                    18,
                    period().value(18).start("2025-07-15T00:00:00Z".parse()?).end("2025-08-15T00:00:00Z".parse()?)
                )
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2025-08-15T00:00:00Z".parse()?, "2026-01-15T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<ContentRange>(), absent());
    let retrieved_2 = &headed_2.outcome;
    let aggregated_2 = retrieved_2.body();
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                len(eq(2)),
                nth(0, period().value(19).start("2025-08-15T00:00:00Z".parse()?)),
                nth(1, period().value(20).end("2025-10-15T00:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
