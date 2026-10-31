//! An emptied map empties a body selector.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::answered, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Entity, NgsiLdMediaType, OneOrMany, Property, Value},
    query::{EntitySelector, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities through POST with an `EntitySelector` naming three ids answers all three without an
/// `EntityMap` and none under an `EntityMap` created through POST over an id no Entity has
/// Clause: 5.5.14, 5.2.33, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_pag_9012_an_emptied_map_empties_a_body_selector() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut ids = Vec::new();
    let mut entities = Vec::new();
    for n in 1..=3 {
        let id = namespace.urn("Building", n)?;
        entities.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(building_type.clone())
                .property("temperature", Property::builder(Value::Number("30.0".parse()?)))
                .build()?,
        );
        ids.push(id);
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(OneOrMany::try_from(ids.clone())?)])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    for id in &ids {
        expect_that!(page.items(), contains(has_id(id)));
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json
        .with_heads()
        .entity_maps()
        .create()
        .selectors([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(namespace.urn("Building", "absent")?)])
        .method(QueryMethod::Post)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(&created.body().entity_map, len(eq(0)));
    let map_id = created.body().id.clone();
    let entity_map_request = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let under_map = json
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request)
        .selectors([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(OneOrMany::try_from(ids.clone())?)])
        .send()
        .await?;
    expect_that!(under_map.items(), len(eq(0)));
    let deleted = broker.with_responses().entity_maps().delete(&map_id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    Ok(())
}
