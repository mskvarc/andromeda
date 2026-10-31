//! An observed at window over a removed scope keeps the removal without observed at.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        Property,
        evolution::{ScopeInstance, ScopeValue},
    },
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Query Temporal Evolution of Entities on timeproperty `observedAt` over a Vehicle whose Scope was removed
/// answers the removal instance of the Scope with `deletedAt` and no `observedAt`
/// Clause: 4.18, 4.5.7, 5.7.4
/// Recorded: clause 4.18 makes a Scope valid from when it is set until it is explicitly removed and gives it no
/// position on the `observedAt` axis of a window; Andromeda keeps the whole Scope history in every window and renders
/// a removal without `observedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9032_an_observed_at_window_over_a_removed_scope_keeps_the_removal_without_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2024-01-01T00:00:00Z".parse()?))
        .scope(format!("/{namespace}/A").as_str())
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().replace(&vehicle_replacement).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let scope = page.items().first().map(|entity| &entity.scope).or_fail()?;
    expect_that!(scope, len(eq(2)));
    expect_that!(scope.first().and_then(ScopeInstance::observed_at), some(anything()));
    expect_that!(scope.get(1).map(ScopeInstance::value), some(eq(&ScopeValue::Removed)));
    expect_that!(scope.get(1).and_then(|instance| instance.output().deleted_at), some(anything()));
    expect_that!(scope.get(1).and_then(ScopeInstance::observed_at), none());
    Ok(())
}
