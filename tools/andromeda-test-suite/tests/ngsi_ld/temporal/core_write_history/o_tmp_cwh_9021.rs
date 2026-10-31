//! A scope restated after its removal records it again.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
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

/// Pins: A Vehicle created with scope /<ns>/A, replaced without scope and then merged with scope /<ns>/A holds
/// three scope instances in its Temporal Evolution: /<ns>/A twice and, between them, the NGSI-LD Null of the
/// removal
/// Clause: 4.18, 4.3.1, 4.5.6, 5.6.17.4, 5.6.18.4
/// Recorded: clause 4.18 does not say whether a write restating the Scope in force sets it again; Andromeda reads a
/// restatement as setting nothing, so it records no scope instance, and clause 4.3.1 leaves it to the broker
/// whether a Core API write updates the Temporal Evolution ("should consider updating"); Andromeda records Core
/// writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9021_a_scope_restated_after_its_removal_records_it_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_removed_restated_id = namespace.urn("Vehicle", "removed-restated")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_removed_restated = Entity::builder()
        .id(vehicle_removed_restated_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(format!("/{namespace}/A").as_str())
        .build()?;
    broker.entities().create(&vehicle_removed_restated).send().await?.done()?;
    let vehicle_removed_restated_replacement = Entity::builder()
        .id(vehicle_removed_restated_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    broker.entities().replace(&vehicle_removed_restated_replacement).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&vehicle_removed_restated_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_removed_restated_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, len(eq(3)));
    expect_that!(
        evolution.scope.first().map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?))))
    );
    expect_that!(evolution.scope.get(1).map(ScopeInstance::value), some(eq(&ScopeValue::Removed)));
    expect_that!(evolution.scope.get(1).and_then(|instance| instance.output().deleted_at), some(anything()));
    expect_that!(
        evolution.scope.get(2).map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?))))
    );
    Ok(())
}
