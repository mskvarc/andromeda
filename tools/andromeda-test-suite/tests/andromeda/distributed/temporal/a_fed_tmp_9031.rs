//! An aggregated retrieval of an entity held in more than one part counts the instances of every part.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    held_retrieval::{ALONE_AGGREGATED, SOURCES, SourceAnswers, expect_aggregate, hold_vehicles, release, retrieval_path},
    namespace::Namespace,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;

/// Pins: Retrieve Temporal Evolution of an Entity as aggregatedValues totalCount and sum, under the default
/// `federated_aggregation` (`overlapping-instances`): a Vehicle the broker holds 30 instances of and one source 30
/// more counts 60 summing to 6870, one three sources hold 30 instances each counts 90 summing to 10305, one only the
/// broker holds counts its own 30 summing to 6435, and one only a source holds keeps that source's aggregate of 7
/// summing to 21; only the Vehicles held in more than one part are asked again for their instances
/// Clause: 5.7.3.4, 4.5.19
/// Recorded: the broker asks the sources for their aggregates first and aggregates here only an entity more than one
/// part answered
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9031_an_aggregated_retrieval_counts_every_part_of_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let held = hold_vehicles(&broker, &source, &namespace, SourceAnswers::AggregatesAndInstances).await?;
    expect_aggregate(&broker, &held.broker_and_one_source, 60, 6870).await?;
    expect_aggregate(&broker, &held.every_source, 90, 10_305).await?;
    expect_aggregate(&broker, &held.broker_alone, 30, 6435).await?;
    expect_aggregate(&broker, &held.one_source_alone, ALONE_AGGREGATED, 21).await?;
    let asked_again = |prefix: &str, vehicle_id| SourceRequest::new(Method::GET, &retrieval_path(prefix, vehicle_id)).without_query("format");
    expect_that!(
        source.received_matching(&asked_again(SOURCES[0], &held.broker_and_one_source)).await?,
        len(eq(1))
    );
    for prefix in SOURCES {
        expect_that!(source.received_matching(&asked_again(prefix, &held.every_source)).await?, len(eq(1)));
    }
    expect_that!(source.received_matching(&asked_again(SOURCES[0], &held.one_source_alone)).await?, is_empty());
    release(&broker, &held).await
}
