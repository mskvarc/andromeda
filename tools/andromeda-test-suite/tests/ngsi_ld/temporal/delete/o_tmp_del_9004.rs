//! A core write restating the scope after delete temporal evolution records no scope.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Property, Scope, ScopeChange},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: After Delete Temporal Evolution of a Vehicle created with scope /<ns>/A, a Merge Entity restating /<ns>/A
/// beside a new speed records the speed instance and no scope instance, so `scopeQ` /<ns>/A selects nothing
/// Clause: 4.18, 4.3.1, 5.8.6, 5.6.16, 5.6.17.4, 5.7.4.4
/// Recorded: clause 4.3.1 updates the Temporal Evolution when the current state "is modified" via the Core API, and
/// clause 4.18 does not say whether a write restating the Scope in force sets it again; Andromeda records a Core
/// write only for what it modified, judged against the current state, so a Scope the current state already held is
/// not recorded even after Delete Temporal Evolution removed it from the history; the Temporal API is the way to
/// state it again
#[gtest]
#[tokio::test]
async fn o_tmp_del_9004_a_core_write_restating_the_scope_after_delete_temporal_evolution_records_no_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reestablished_id = namespace.urn("Vehicle", "reestablished")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_reestablished = Entity::builder()
        .id(vehicle_reestablished_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(format!("/{namespace}/A").as_str())
        .property("speed", Property::builder(1).observed_at("2099-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_reestablished).send().await?.done()?;
    broker.temporal().delete(&vehicle_reestablished_id).send().await?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?)))
        .property("speed", Property::builder(2).observed_at("2099-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().merge(&vehicle_reestablished_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reestablished_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, is_empty());
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(2)))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q(format!("/{namespace}/A").as_str().parse::<ScopeQuery>()?)
        .ids(vec![vehicle_reestablished_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
