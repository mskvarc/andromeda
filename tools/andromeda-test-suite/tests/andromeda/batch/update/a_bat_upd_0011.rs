//! A respond async update is answered by q at once and never over a later write.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    respond_async::{async_applied, respond_async},
    values_matcher::has_values,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{BatchUpdate, ResultsCount},
    },
    model::{Entity, EntityFragment, EntityTypeName, NgsiLdMediaType},
    query::{
        q,
        temporal_query::{self, TemporalProperty},
    },
};

/// The number of sensors the test writes.
const SENSORS: u64 = 8;

/// Asserts that Query Entities of `sensor_type` with `q` level==`level` counts and answers `expected` entities.
async fn expect_selected(broker: &BrokerClient, sensor_type: &EntityTypeName, level: i32, expected: u64) -> Result<()> {
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(sensor_type)
        .q(q::attr("level").eq(level))
        .count(ResultsCount::Include)
        .limit(100)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(expected)));
    expect_that!(page.items(), len(eq(usize::try_from(expected)?)));
    Ok(())
}

/// Pins: after a batch update answered with Preference-Applied: respond-async, q selects the Entities by the values
/// the update wrote at once and no longer by the values it replaced; a synchronous update that follows is what q
/// selects by, then and afterwards
/// Clause: 4.9, 5.7.2.4
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1)
#[gtest]
#[tokio::test]
async fn a_bat_upd_0011_a_respond_async_update_is_answered_by_q_at_once_and_never_over_a_later_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_ids = (1..=SENSORS).map(|n| namespace.urn("Sensor", n)).collect::<Result<Vec<_>, _>>()?;
    let sensors = sensor_ids
        .iter()
        .map(|id| Entity::builder().id(id.clone()).entity_type(sensor_type.clone()).property("level", 1).build())
        .collect::<Result<Vec<_>, _>>()?;
    broker.entities().batch_create(&sensors).send().await?.done()?;
    let fragments = |level: i32| {
        sensor_ids
            .iter()
            .map(|id| EntityFragment::for_entity(id.clone(), sensor_type.clone()).property("level", level).build())
            .collect::<Result<Vec<_>, _>>()
    };
    let async_fragments = fragments(2)?;
    let request = respond_async(broker.unchecked(&BatchUpdate::builder().fragments(&async_fragments).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    // Asked at once, with no retry: the update was accepted, so every Entity holds 2 and none holds 1 any more (clause
    // 5.7.2.4, "the filter conditions specified by the query are met").
    expect_selected(&broker, &sensor_type, 1, 0).await?;
    expect_selected(&broker, &sensor_type, 2, SENSORS).await?;
    // Sent straight after the deferred work was queued, so the deferred work may still run after it; whichever order
    // they run in, the later write is the one the Entities hold.
    let later_fragments = fragments(3)?;
    broker.entities().batch_update(&later_fragments).send().await?.done()?;
    expect_selected(&broker, &sensor_type, 3, SENSORS).await?;
    // The deferred work records the respond-async update's observation of 2 in the Temporal Evolution, so three
    // instances per Entity mean it has run, in whichever order against the later write; the instances carry no
    // observedAt, so the query selects them by modifiedAt.
    retried(async || {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query_values(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
            .entity_type(&sensor_type)
            .attrs(vec!["level".parse()?])
            .limit(100)
            .send()
            .await?;
        verify_that!(page.items(), len(eq(usize::try_from(SENSORS)?)))?;
        verify_that!(page.items(), each(has_values("level", len(eq(3)))))?;
        Ok(())
    })
    .await?;
    // Asked again once the deferred work has run: it must not bring 2 back over the later write.
    expect_selected(&broker, &sensor_type, 2, 0).await?;
    expect_selected(&broker, &sensor_type, 3, SENSORS).await?;
    Ok(())
}
