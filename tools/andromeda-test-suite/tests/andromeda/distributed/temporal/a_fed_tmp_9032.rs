//! An aggregated retrieval aggregates every instance its sources answered with those the broker holds.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    context_source::ContextSource,
    held_retrieval::{SOURCES, SourceAnswers, expect_aggregate, hold_vehicles, release, retrieval_path},
    namespace::Namespace,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;

/// Pins: Retrieve Temporal Evolution of an Entity as aggregatedValues totalCount and sum, on a broker whose
/// `federated_aggregation` is `instances`: a Vehicle the broker holds 30 instances of and one source 30 more counts 60
/// summing to 6870, one three sources hold 30 instances each counts 90 summing to 10305, one only the broker holds
/// counts its own 30 summing to 6435, and one only a source holds counts that source's 30 summing to 435; no source is
/// asked for an aggregate
/// Clause: 5.7.3.4, 4.5.19
/// Recorded: the broker asks the sources for every instance of the window in one round and aggregates here
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9032_an_aggregated_retrieval_aggregates_every_answered_instance_here() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Edge)?;
    let source = ContextSource::start().await?;
    let held = hold_vehicles(&broker, &source, &namespace, SourceAnswers::Instances).await?;
    expect_aggregate(&broker, &held.broker_and_one_source, 60, 6870).await?;
    expect_aggregate(&broker, &held.every_source, 90, 10_305).await?;
    expect_aggregate(&broker, &held.broker_alone, 30, 6435).await?;
    expect_aggregate(&broker, &held.one_source_alone, 30, 435).await?;
    for prefix in SOURCES {
        let vehicles = [&held.broker_and_one_source, &held.every_source, &held.broker_alone, &held.one_source_alone];
        for vehicle_id in vehicles {
            let aggregate_asked = SourceRequest::new(Method::GET, &retrieval_path(prefix, vehicle_id)).query("format", "aggregatedValues");
            expect_that!(source.received_matching(&aggregate_asked).await?, is_empty());
        }
    }
    release(&broker, &held).await
}
