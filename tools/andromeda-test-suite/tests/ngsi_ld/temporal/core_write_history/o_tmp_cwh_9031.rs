//! A removed scope is placed by deleted at and renders no observed at.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        OneOrMany,
        Scope,
        ScopeChange,
        evolution::{ScopeInstance, ScopeValue},
    },
};

/// Pins: A Vehicle created with scope /<ns>/A, replaced without scope and merged with scope /<ns>/A again holds a
/// removal instance of the Scope with value urn:ngsi-ld:null and `deletedAt` and no `observedAt`, in the normalized,
/// `temporalValues` and `sysAttrs` representations, while the instances that set the Scope keep their `observedAt`
/// Clause: 4.18, 4.5.7, 4.5.9
/// Recorded: clause 4.5.7 gives a deleted instance the instant of its `deletedAt` and clause 4.18 makes a Scope valid
/// until it is explicitly removed; the spec does not say whether a removal of the Scope renders an `observedAt`, and
/// Andromeda renders none
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9031_a_removed_scope_is_placed_by_deleted_at_and_renders_no_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(format!("/{namespace}/A").as_str())
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 40)
        .build()?;
    broker.entities().replace(&vehicle_replacement).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.temporal().retrieve(&vehicle_id).send().await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, len(eq(3)));
    expect_that!(evolution.scope.first().and_then(ScopeInstance::observed_at), some(anything()));
    expect_that!(evolution.scope.get(1).map(ScopeInstance::value), some(eq(&ScopeValue::Removed)));
    expect_that!(evolution.scope.get(1).and_then(|instance| instance.output().deleted_at), some(anything()));
    expect_that!(evolution.scope.get(1).and_then(ScopeInstance::observed_at), none());
    expect_that!(evolution.scope.get(2).and_then(ScopeInstance::observed_at), some(anything()));
    let retrieved_2 = json
        .temporal()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2.scope, len(eq(3)));
    expect_that!(evolution_2.scope.get(1).and_then(|instance| instance.output().deleted_at), some(anything()));
    expect_that!(evolution_2.scope.get(1).and_then(ScopeInstance::observed_at), none());
    expect_that!(evolution_2.scope.get(1).and_then(|instance| instance.output().created_at), none());
    expect_that!(evolution_2.scope.get(1).and_then(|instance| instance.output().modified_at), none());
    let retrieved_3 = json.temporal().retrieve_values(&vehicle_id).send().await?;
    let values = retrieved_3.body();
    expect_that!(values.scope, len(eq(3)));
    expect_that!(values.scope.get(1), some(anything()));
    Ok(())
}
