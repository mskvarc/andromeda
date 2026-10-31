//! The reified scope is admitted in attrs.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Scope,
        evolution::{ScopeInstance, ScopeValue},
    },
    query::{projection::Projection, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with attrs=speed,scope answers the Scope as a reified Property beside
/// speed, and pick=id,type,speed,scope answers the same Scope
/// Clause: Table 6.18.3.2-1, 4.18, 5.7.4.4
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9001_the_reified_scope_is_admitted_in_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let madrid_centro = ScopeValue::Scopes(OneOrMany::one("/Madrid/Centro".parse::<Scope>()?));
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid/Centro")
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .attrs(vec!["speed".parse()?, "scope".parse()?])
        .send()
        .await?;
    let entity = page.items().first().or_fail()?;
    expect_that!(entity.scope.first().map(ScopeInstance::value), some(eq(&madrid_centro)));
    expect_that!(entity, has_evolution("speed", contains(any_attribute().value(60))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_projected(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id])
        .pick("id,type,speed,scope".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = page_2.items().first().or_fail()?;
    expect_that!(entity_2.scope.first().map(ScopeInstance::value), some(eq(&madrid_centro)));
    Ok(())
}
