//! A scope query sheds a mapped entity moved out of scope.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    received_status::ExpectedStatus,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::scope_query::ScopeQuery,
};

/// Pins: Query Entities by type and `scopeQ` `/AreaA` under an `EntityMap` of four Entities in that scope, after
/// the first page of two, answers a later page without the Entity replaced into `/AreaB`, and the `EntityMap` no
/// longer lists it
/// Clause: 4.3.6.7, 5.5.9.3, 5.5.14
#[gtest]
#[tokio::test]
async fn n_qry_pag_9002_a_scope_query_sheds_a_mapped_entity_moved_out_of_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut entities = Vec::new();
    for n in 1..=4 {
        entities.push(
            Entity::builder()
                .id(namespace.urn("Building", n)?)
                .entity_type(building_type.clone())
                .scope("/AreaA")
                .property("temperature", Property::builder(Value::Number("30.0".parse()?)))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&building_type)
        .scope_q("/AreaA".parse::<ScopeQuery>()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(&created.body().entity_map, len(eq(4)));
    let map_id = created.body().id.clone();
    let map_request = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/AreaA".parse::<ScopeQuery>()?)
        .limit(2)
        .offset(0)
        .entity_map(&map_request)
        .send()
        .await?;
    expect_that!(first.items(), len(eq(2)));
    // The third Entity of the sequence is one the first page did not offer, so it is checked again only by the pages
    // read after it stops fitting.
    let third = json
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/AreaA".parse::<ScopeQuery>()?)
        .limit(1)
        .offset(2)
        .entity_map(&map_request)
        .send()
        .await?;
    expect_that!(third.items(), len(eq(1)));
    let victim = third.items().first().or_fail()?.id.clone();
    // Replace Entity takes an entity with an id, where this body leaves the id to the path.
    let replacement = format!(r#"{{"type": "{building_type}", "scope": "/AreaB", "temperature": {{"type": "Property", "value": 30.0}}}}"#);
    let request = json_request_to(&broker, Method::PUT, &format!("entities/{victim}"), &replacement)?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let rest = json
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/AreaA".parse::<ScopeQuery>()?)
        .limit(100)
        .offset(0)
        .entity_map(&map_request)
        .send()
        .await?;
    expect_that!(rest.items(), len(eq(3)));
    expect_that!(rest.items(), not(contains(has_id(&victim))));
    let map = json.entity_maps().retrieve(&map_id).send().await?;
    expect_that!(&map.entity_map, len(eq(3)));
    expect_that!(map.entity_map.contains_key(&victim), eq(false));
    broker.entity_maps().delete(&map_id).send().await?;
    Ok(())
}
