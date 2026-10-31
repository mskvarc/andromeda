//! Q on modified at admits the instances in effect when the window opens.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_lookup::with_id,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    power_meters::{append_status, instants, meter_id, write_power},
};
use chrono::Utc;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{
        q,
        temporal_query::{self, TemporalProperty},
    },
};
use std::time::Duration;
use tokio::time::sleep;

/// A wall-clock second plus a margin, so an instant taken after it lies in a later second than any earlier write.
const ONE_SECOND: Duration = Duration::from_millis(1100);

/// Pins: a temporal q on `timeproperty=modifiedAt` reads the instances in effect when an after window opens: every
/// power instance and the deletion were written before it, so each entity is judged by its last power write, all of
/// whose instances are in effect, and the entity whose last write is a deletion is left out
/// Clause: 5.7.4.4, 4.11, 4.8, 5.2.21, 4.9, 4.5.7
/// Recorded: clause 5.7.4.4 checks q against the Attribute instances "during the interval defined by the temporal
/// query" and clause 4.11 makes them the values "valid within the specified timeframe", without saying whether the
/// instance observed before the window opens and not yet replaced is one of them; Andromeda reads it as one (as
/// Annex C clause C.5.5 evaluates a q on an attribute it does not render), evaluated by q but never rendered
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0004_q_on_modified_at_admits_the_instances_in_effect_when_the_window_opens() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let at = instants()?;
    write_power(&broker, &namespace, &at).await?;
    // The instant between the writes is taken a whole second after the first ones and the next writes a whole second
    // after it, so each side differs in the second.
    sleep(ONE_SECOND).await;
    let between = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string().parse()?;
    sleep(ONE_SECOND).await;
    append_status(&broker, &namespace, &at).await?;

    let meter_type = namespace.entity_type("Meter")?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let page = json
        .temporal()
        .query(temporal_query::after(between).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&meter_type)
        .q(q::attr("power").gt(8))
        .attrs(vec!["status".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    for suffix in ["M1", "M2", "M4", "M5", "M6"] {
        let entity = with_id(page.items(), &meter_id(&namespace, suffix)?).or_fail()?;
        expect_that!(entity.attributes.len(), eq(1));
        expect_that!(entity, has_evolution("status", all!(len(eq(1)), nth(0, any_attribute().observed_at(at.t75)))));
    }
    Ok(())
}
