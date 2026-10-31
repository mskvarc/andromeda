//! A merge restating the scope records no scope instance.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Scope,
        ScopeChange,
        evolution::{ScopeInstance, ScopeValue},
    },
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Three Merge Entity requests restating scope /<ns>/A beside a new speed leave the Temporal Evolution of a
/// Vehicle created with that scope holding one scope instance, observed when the creation set it, and `scopeQ`
/// /<ns>/A selects all four speed instances
/// Clause: 4.18, 4.3.1, 4.5.6, 5.6.17.4, 5.7.4.4, C.5.16
/// Recorded: clause 4.18 does not say whether a write restating the Scope in force sets it again; Andromeda reads a
/// restatement as setting nothing, so it records no scope instance, and clause 4.3.1 leaves it to the broker
/// whether a Core API write updates the Temporal Evolution ("should consider updating"); Andromeda records Core
/// writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9019_a_merge_restating_the_scope_records_no_scope_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_restated_id = namespace.urn("Vehicle", "restated")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let scope_a = format!("/{namespace}/A").parse::<Scope>()?;
    let vehicle_restated = Entity::builder()
        .id(vehicle_restated_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(format!("/{namespace}/A").as_str())
        .property("speed", Property::builder(1).observed_at("2099-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_restated).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json.temporal().retrieve(&vehicle_restated_id).send().await?;
    expect_that!(created.body().scope, len(eq(1)));
    let set_at = created.body().scope.first().and_then(ScopeInstance::observed_at).copied().or_fail()?;
    for (speed, instant) in [(2, "2099-01-01T00:01:00Z"), (3, "2099-01-01T00:02:00Z"), (4, "2099-01-01T00:03:00Z")] {
        let fragment = EntityFragment::builder()
            .scope(ScopeChange::Set(OneOrMany::one(scope_a.clone())))
            .property("speed", Property::builder(speed).observed_at(instant.parse()?))
            .build()?;
        broker.entities().merge(&vehicle_restated_id, &fragment).send().await?.done()?;
    }
    let restated = json.temporal().retrieve(&vehicle_restated_id).send().await?;
    let evolution = restated.body();
    expect_that!(evolution.scope, len(eq(1)));
    expect_that!(
        evolution.scope.first().map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(scope_a))))
    );
    expect_that!(evolution.scope.first().and_then(ScopeInstance::observed_at), some(eq(&set_at)));
    expect_that!(evolution, has_evolution("speed", len(eq(4))));
    let page = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q(format!("/{namespace}/A").as_str().parse::<ScopeQuery>()?)
        .ids(vec![vehicle_restated_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first(), some(has_evolution("speed", len(eq(4)))));
    Ok(())
}
