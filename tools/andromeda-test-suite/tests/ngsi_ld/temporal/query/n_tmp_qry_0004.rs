//! Q on modified at reads only the instances written inside the window.

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

/// Pins: with `timeproperty=modifiedAt` and a window opening after every power instance was written and before the
/// status instances were, a temporal q power>8 selects no meter, since no power instance was modified inside the
/// window, while q status=="on" selects all six, each answering its one status instance
/// Clause: 5.7.4.4, 4.11, 4.8, 5.2.21, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0004_q_on_modified_at_reads_only_the_instances_written_inside_the_window() -> Result<()> {
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
    let by_power = json
        .temporal()
        .query(temporal_query::after(between).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&meter_type)
        .q(q::attr("power").gt(8))
        .attrs(vec!["status".parse()?])
        .send()
        .await?;
    expect_that!(by_power.items(), is_empty());
    let by_status = json
        .temporal()
        .query(temporal_query::after(between).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&meter_type)
        .q(q::attr("status").eq("on"))
        .attrs(vec!["status".parse()?])
        .send()
        .await?;
    expect_that!(by_status.items(), len(eq(6)));
    for suffix in ["M1", "M2", "M3", "M4", "M5", "M6"] {
        let entity = with_id(by_status.items(), &meter_id(&namespace, suffix)?).or_fail()?;
        expect_that!(entity.attributes.len(), eq(1));
        expect_that!(entity, has_evolution("status", all!(len(eq(1)), nth(0, any_attribute().observed_at(at.t75)))));
    }
    Ok(())
}
