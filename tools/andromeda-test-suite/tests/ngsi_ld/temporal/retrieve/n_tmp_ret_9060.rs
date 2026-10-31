//! Last n skips the deletion record of a deleted attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, DateTime, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalProperty,
};

/// Pins: speed holding 10 and 20 observed at 2024-01-01T00:00:00Z and 01:00 and then deleted through the Core API
/// answers lastN 1 with 20, lastN 10 with 10 and 20, and lastN 1 on timeproperty deletedAt with the one instance holding
/// the NGSI-LD Null
/// Clause: 5.7.3.4, 4.5.7, 4.11, 5.6.5
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9060_last_n_skips_the_deletion_record_of_a_deleted_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let first = "2024-01-01T00:00:00Z".parse::<DateTime>()?;
    let second = "2024-01-01T01:00:00Z".parse::<DateTime>()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at(first))
        .property("speed", Property::builder(20).observed_at(second))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let last = client.temporal().retrieve(&vehicle_id).last_n(1).send().await?;
    expect_that!(
        last.body(),
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20).observed_at(second))))
    );
    let all = client.temporal().retrieve(&vehicle_id).last_n(10).send().await?;
    expect_that!(
        all.body(),
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(10)), nth(1, any_attribute().value(20))))
    );
    let deleted = client
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::DeletedAt)
        .last_n(1)
        .send()
        .await?;
    expect_that!(deleted.body(), has_evolution("speed", all!(len(eq(1)), nth(0, deleted_instance()))));
    Ok(())
}
