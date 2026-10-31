//! Each temporal property selects by its own instant.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    deleted_instance::deleted_instance,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, Property},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Speed observed at 2021-01-01 and then deleted through the Core API: the observedAt window around 2021-01-01
/// holds the observation 55, the same window on createdAt, modifiedAt and deletedAt is 404 `ResourceNotFound`, and since
/// 2026-01-01 createdAt and modifiedAt each hold the observation 55 and deletedAt holds the deletion alone
/// Clause: 4.11, 5.2.21, 5.7.3.4, 4.5.7
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9038_each_temporal_property_selects_by_its_own_instant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_axes_id = namespace.urn("Vehicle", "axes")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_axes = Entity::builder()
        .id(vehicle_axes_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_axes).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&vehicle_axes_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let around = || -> Result<_> { Ok(temporal_query::between("2020-12-31T23:59:59Z".parse()?, "2021-01-01T00:00:01Z".parse()?)) };
    let observed = client.temporal().retrieve(&vehicle_axes_id).temporal_q(around()?).send().await?;
    expect_that!(observed.body(), has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(55)))));
    for property in [TemporalProperty::CreatedAt, TemporalProperty::ModifiedAt, TemporalProperty::DeletedAt] {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&vehicle_axes_id)
            .temporal_q(around()?.time_property(property))
            .send()
            .await;
        expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    for property in [TemporalProperty::CreatedAt, TemporalProperty::ModifiedAt] {
        let since = client
            .temporal()
            .retrieve(&vehicle_axes_id)
            .temporal_q(temporal_query::after("2026-01-01T00:00:00Z".parse()?).time_property(property))
            .send()
            .await?;
        expect_that!(since.body(), has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(55)))));
    }
    let deleted = client
        .temporal()
        .retrieve(&vehicle_axes_id)
        .temporal_q(temporal_query::after("2026-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::DeletedAt))
        .send()
        .await?;
    expect_that!(deleted.body(), has_evolution("speed", all!(len(eq(1)), nth(0, deleted_instance()))));
    Ok(())
}
