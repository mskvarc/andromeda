//! A scope changed and changed back records three scope instances.

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

/// Pins: A Vehicle created with scope /<ns>/A, merged to /<ns>/B and merged back to /<ns>/A holds three scope
/// instances in its Temporal Evolution, two of /<ns>/A and one of /<ns>/B
/// Clause: 4.18, 4.3.1, 4.5.6, 5.6.17.4
/// Recorded: clause 4.18 does not say whether a write restating the Scope in force sets it again; Andromeda reads a
/// restatement as setting nothing, so it records no scope instance, and clause 4.3.1 leaves it to the broker
/// whether a Core API write updates the Temporal Evolution ("should consider updating"); Andromeda records Core
/// writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9020_a_scope_changed_and_changed_back_records_three_scope_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_changed_back_id = namespace.urn("Vehicle", "changed-back")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_changed_back = Entity::builder()
        .id(vehicle_changed_back_id.clone())
        .entity_type(vehicle_type)
        .scope(format!("/{namespace}/A").as_str())
        .build()?;
    broker.entities().create(&vehicle_changed_back).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{namespace}/B").parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&vehicle_changed_back_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&vehicle_changed_back_id, &fragment_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_changed_back_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, len(eq(3)));
    expect_that!(
        evolution.scope.first().map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?))))
    );
    expect_that!(
        evolution.scope.get(1).map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/B").parse::<Scope>()?))))
    );
    expect_that!(
        evolution.scope.get(2).map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?))))
    );
    Ok(())
}
