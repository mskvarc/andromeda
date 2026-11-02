//! Every gauge stays in the scrape for as long as its value holds.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, DEFAULT_TENANT_LABEL, MetricLabel, MetricsReading},
    config::BrokerRole,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::model::Entity;
use std::time::Duration;
use tokio::time::sleep;

/// Past the broker's five-minute gauge idle timeout, with a minute's margin.
const PAST_THE_GAUGE_IDLE_TIMEOUT: Duration = Duration::from_secs(360);

/// The route template of Create Entity, the only route the test sends a request to.
const ENTITIES_ROUTE: &str = "/ngsi-ld/v1/entities";

/// The gauges the default tenant's table sampling publishes.
const TABLE_GAUGES: [BrokerMetric; 4] = [
    BrokerMetric::TableBytes,
    BrokerMetric::TableIndexBytes,
    BrokerMetric::TableHotUpdateRatio,
    BrokerMetric::HistoryLargestRowstoreChunkBytes,
];

/// Pins: right after one Create Entity into the default tenant of a broker no other test talks to, and again six
/// minutes later with no request in between, its scrape exposes `andromeda_tenant_runtimes_cached` and
/// `andromeda_name_dictionary_entries` at 1 or more, `andromeda_http_requests_pending` of the entities route at 0, and
/// every table size gauge of the default tenant
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the Prometheus metrics on the management port
#[gtest]
#[tokio::test]
async fn a_obs_met_9001_every_gauge_stays_in_the_scrape_for_as_long_as_its_value_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let observed = broker::client(BrokerRole::Observed)?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(namespace.entity_type("Building")?)
        .property("height", 12)
        .build()?;
    observed.entities().create(&building).send().await?.done()?;
    let after_the_write = MetricsReading::of(BrokerRole::Observed).await?;
    sleep(PAST_THE_GAUGE_IDLE_TIMEOUT).await;
    let past_the_timeout = MetricsReading::of(BrokerRole::Observed).await?;
    for (when, reading) in [("after the write", &after_the_write), ("past the idle timeout", &past_the_timeout)] {
        expect_that!(reading.sum(BrokerMetric::RuntimesCached), some(ge(1.0)), "runtimes cached {when}");
        expect_that!(
            reading.sum(BrokerMetric::NameDictionaryEntries),
            some(ge(1.0)),
            "name dictionary entries {when}"
        );
        expect_that!(
            reading.sum_labelled(BrokerMetric::HttpRequestsPending, MetricLabel::Endpoint, ENTITIES_ROUTE),
            some(eq(0.0)),
            "requests pending on {ENTITIES_ROUTE} {when}"
        );
        for gauge in TABLE_GAUGES {
            expect_that!(
                reading.sum_labelled(gauge, MetricLabel::Tenant, DEFAULT_TENANT_LABEL),
                some(anything()),
                "{gauge:?} of the default tenant {when}"
            );
        }
    }
    Ok(())
}
