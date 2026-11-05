//! Every gauge stays in the scrape for as long as its value holds.

use andromeda_test_suite::{
    broker,
    broker_metrics::{BrokerMetric, DEFAULT_TENANT_LABEL, MetricLabel, MetricsReading},
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{model::Entity, query::q};
use std::time::Duration;
use tokio::time::sleep;

/// Past the broker's five-minute gauge idle timeout, with a minute's margin.
const PAST_THE_GAUGE_IDLE_TIMEOUT: Duration = Duration::from_secs(360);

/// The route template of Create Entity and Query Entities, the only route the test sends requests to.
const ENTITIES_ROUTE: &str = "/ngsi-ld/v1/entities";

/// The series the default tenant's table sampling publishes.
const TABLE_SERIES: [BrokerMetric; 5] = [
    BrokerMetric::TableBytes,
    BrokerMetric::TableIndexBytes,
    BrokerMetric::TableUpdates,
    BrokerMetric::TableHotUpdates,
    BrokerMetric::HistoryLargestRowstoreChunkBytes,
];

/// Pins: right after one Create Entity into the default tenant of a broker no other test talks to and one query of its
/// type and attribute, and again six minutes later with no request in between, its scrape exposes
/// `andromeda_tenant_runtimes_cached` and `andromeda_name_dictionary_entries` at 1 or more,
/// `andromeda_http_requests_pending` of the entities route at 0, and every table series of the default tenant
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the Prometheus metrics on the management port
///
/// The names a write mints are not cached, since its transaction could still roll back; the query resolves them from
/// the committed dictionary, which caches them, so the entries gauge counts them.
#[gtest]
#[tokio::test]
async fn a_obs_met_9001_every_gauge_stays_in_the_scrape_for_as_long_as_its_value_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let observed = broker::client(BrokerRole::Observed)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("height", 12)
        .build()?;
    observed.entities().create(&building).send().await?.done()?;
    let page = observed
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("height").eq(12))
        .send()
        .await?;
    expect_that!(page.items(), elements_are![has_id(&building_id)]);
    let after_the_query = MetricsReading::of(BrokerRole::Observed).await?;
    sleep(PAST_THE_GAUGE_IDLE_TIMEOUT).await;
    let past_the_timeout = MetricsReading::of(BrokerRole::Observed).await?;
    for (when, reading) in [("after the query", &after_the_query), ("past the idle timeout", &past_the_timeout)] {
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
        for series in TABLE_SERIES {
            expect_that!(
                reading.sum_labelled(series, MetricLabel::Tenant, DEFAULT_TENANT_LABEL),
                some(anything()),
                "{series:?} of the default tenant {when}"
            );
        }
    }
    Ok(())
}
