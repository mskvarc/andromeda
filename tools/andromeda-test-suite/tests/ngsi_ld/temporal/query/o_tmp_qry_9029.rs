//! A query page is cut at one instant across entities.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{DateTime, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities over an Entity with 30 instances from 2024-01-01 and one with 3 from
/// 2024-07-19 answers 206 with both, the first holding 19 instances and the second an empty speed, and one Content-Range
/// ending at the 19th instant
/// Clause: 5.7.4.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// lastN and with lastN (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 gives a response one Content-Range; Andromeda cuts every Entity of the page at that one
/// instant and keeps an Entity whose instances all lie beyond it, its attribute empty
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9029_a_query_page_is_cut_at_one_instant_across_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_fleet_early_id = namespace.urn("PagedFleet", "early")?;
    let paged_fleet_type = namespace.entity_type("PagedFleet")?;
    let paged_fleet_late_id = namespace.urn("PagedFleet", "late")?;
    let paged_fleet_early_evolution = (0..30)
        .try_fold(
            TemporalEntity::builder().id(paged_fleet_early_id.clone()).entity_type(paged_fleet_type.clone()),
            |builder, minute| -> Result<_> {
                Ok(builder.property(
                    "speed",
                    Property::builder(minute).observed_at(format!("2024-01-01T00:{minute:02}:00Z").parse()?),
                ))
            },
        )?
        .build()?;
    broker.temporal().create_or_update(&paged_fleet_early_evolution).send().await?.created()?;
    let paged_fleet_late_evolution = TemporalEntity::builder()
        .id(paged_fleet_late_id.clone())
        .entity_type(paged_fleet_type)
        .property("speed", Property::builder(500).observed_at("2024-07-19T00:00:00Z".parse()?))
        .property("speed", Property::builder(501).observed_at("2024-07-19T00:01:00Z".parse()?))
        .property("speed", Property::builder(502).observed_at("2024-07-19T00:02:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_fleet_late_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let headed = client
        .with_heads()
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&namespace.entity_type("PagedFleet")?)
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    let items = headed.outcome.items();
    expect_that!(items, len(eq(2)));
    let early = items.iter().find(|evolution| evolution.id == paged_fleet_early_id).or_fail()?;
    expect_that!(early, has_evolution("speed", len(eq(19))));
    let late = items.iter().find(|evolution| evolution.id == paged_fleet_late_id).or_fail()?;
    expect_that!(late, has_evolution("speed", len(eq(0))));
    let range = headed.head.typed::<ContentRange>().into_valid().or_fail()?;
    expect_that!(
        (range.start().timestamp(), range.end().timestamp(), range.size()),
        eq((
            "2020-01-01T00:00:00Z".parse::<DateTime>()?.timestamp(),
            "2024-01-01T00:18:00Z".parse::<DateTime>()?.timestamp(),
            ContentRangeSize::Unknown
        ))
    );
    Ok(())
}
