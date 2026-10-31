//! A respond async update is answered by the temporal api at once.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    respond_async::{async_applied, respond_async},
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpdate},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: a batch update answered with Preference-Applied: respond-async is in the Temporal Evolution at once: a
/// temporal query sent straight after it, with no retry, answers every Entity with the instance the update wrote
/// beside the one its creation wrote
/// Clause: 5.7.4
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write and answered by every temporal read before the queue is applied
#[gtest]
#[tokio::test]
async fn a_bat_upd_0013_a_respond_async_update_is_answered_by_the_temporal_api_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_ids = (1..=3).map(|n| namespace.urn("Sensor", n)).collect::<Result<Vec<_>, _>>()?;
    let sensors = sensor_ids
        .iter()
        .map(|id| Entity::builder().id(id.clone()).entity_type(sensor_type.clone()).property("level", 1).build())
        .collect::<Result<Vec<_>, _>>()?;
    broker.entities().batch_create(&sensors).send().await?.done()?;
    let fragments = sensor_ids
        .iter()
        .map(|id| EntityFragment::for_entity(id.clone(), sensor_type.clone()).property("level", 2).build())
        .collect::<Result<Vec<_>, _>>()?;
    let request = respond_async(broker.unchecked(&BatchUpdate::builder().fragments(&fragments).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    // Asked at once, with no retry. Core write history is recorded on this broker, so each Entity holds its
    // creation's instance and the update's; the instances carry no observedAt, so the query selects them by
    // modifiedAt.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_values(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&sensor_type)
        .attrs(vec!["level".parse()?])
        .limit(100)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(
        page.items(),
        each(has_values("level", all!(len(eq(2)), contains(pair().value(1)), contains(pair().value(2)))))
    );
    Ok(())
}
