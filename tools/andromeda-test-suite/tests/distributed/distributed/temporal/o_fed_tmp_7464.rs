//! A split instance page counts only the instances that meet q.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Upserted,
    paged_history::has_range,
    source_answer,
    split_answer::{Observation, Observed::Number, answered_instances, evolution, register_source},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{DateTime, evolution::SimplifiedTemporalEntity},
    query::{EntitySplitting, q::Query, temporal_query},
};

/// The instant `seconds` seconds past noon on 2020-08-01.
fn past_noon(seconds: u32) -> Result<DateTime> {
    Ok(format!("2020-08-01T12:{:02}:{:02}Z", seconds / 60, seconds % 60).parse()?)
}

/// Seventy `speed` instances holding `value`, one every 20 seconds from `first` seconds past noon.
fn every_20_seconds(value: i64, first: u32) -> Result<Vec<Observation>> {
    (0..70)
        .map(|index| {
            Ok(Observation {
                attribute: "speed",
                value: Number(value),
                observed: past_noon(first + 20 * index)?,
            })
        })
        .collect()
}

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true and q speed>90, over a
/// Vehicle whose local speed is 95 every 20 s from 12:00:10 and whose source speed is 95 every 20 s from 12:00:20 and
/// 80 every 20 s from 12:00:15, seventy of each, answers 206 with the first 100 instances of 95, one every 10 s, and
/// a Content-Range from 12:00:00 to 12:16:40, the 100th of them; the request continuing from 12:16:40.000001 answers
/// 200 with the 40 others, and no page carries an instance of 80
/// Clause: 6.3.10, 5.7.4.4
/// Recorded: clause 6.3.10 leaves the page size to the implementation; the front broker pages 100 instances per
/// stream, its `temporal.query.default_last_n`
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7464_a_split_instance_page_counts_only_the_instances_that_meet_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    broker
        .temporal()
        .create_or_update(&evolution(&vehicle_id, &vehicle_type, &every_20_seconds(95, 10)?)?)
        .send()
        .await?
        .created()?;
    let _registration_cleanup = register_source(&broker, &namespace, source.endpoint("a")?).await?;
    let mut held_by_source = every_20_seconds(95, 20)?;
    held_by_source.extend(every_20_seconds(80, 15)?);
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&[evolution(&vehicle_id, &vehicle_type, &held_by_source)?]),
        )
        .await?;
    let speed_above_90: Query = "speed>90".parse()?;
    let first = broker
        .with_accept(Accept::any())
        .with_heads()
        .temporal()
        .query_values(temporal_query::between(past_noon(0)?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(speed_above_90.clone())
        .send()
        .await?;
    expect_that!(first.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        first.head.typed::<ContentRange>().valid(),
        some(has_range(past_noon(0)?, past_noon(1000)?, ContentRangeSize::Unknown))
    );
    let second = broker
        .with_accept(Accept::any())
        .with_heads()
        .temporal()
        .query_values(temporal_query::between("2020-08-01T12:16:40.000001Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(speed_above_90)
        .send()
        .await?;
    expect_that!(second.status, eq(StatusCode::OK));
    expect_that!(second.head.typed::<ContentRange>().valid(), none());
    let expected_first: Vec<DateTime> = (1..=100).map(|tenth| past_noon(10 * tenth)).collect::<Result<_>>()?;
    expect_that!(instants_of(first.outcome.items()), eq(&expected_first));
    let expected_second: Vec<DateTime> = (101..=140).map(|tenth| past_noon(10 * tenth)).collect::<Result<_>>()?;
    expect_that!(instants_of(second.outcome.items()), eq(&expected_second));
    Ok(())
}

/// The instants of every instance `entities` carry, in order.
fn instants_of(entities: &[SimplifiedTemporalEntity]) -> Vec<DateTime> {
    let mut instants: Vec<DateTime> = entities.iter().flat_map(answered_instances).map(|(_, time, _)| time).collect();
    instants.sort_unstable();
    instants
}
