//! A time at and an end time at with a comma fraction are accepted.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities and Retrieve Temporal Evolution accept `timeAt` and `endTimeAt` whose
/// seconds fraction is separated by a comma, and the window selects the instance inside it
/// Clause: 4.6.3, 5.7.3, 5.7.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_2302_a_time_at_and_an_end_time_at_with_a_comma_fraction_are_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(55).observed_at("2020-08-01T12:00:00.5Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00,0Z".parse()?, "2020-08-01T12:00:01,0Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2020-08-01T12:00:00,25Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
