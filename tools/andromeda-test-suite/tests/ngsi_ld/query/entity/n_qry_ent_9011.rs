//! A posted wildcard selector beside another refuses local false.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, EntityId, EntityTypeName, ErrorType, NgsiLdMediaType},
    query::{EntitySelector, Locality, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities by POST with a type * `EntitySelector` beside a named one answers 400 `InvalidRequest` with
/// local false, and 200 with the entities of both selectors alone or with local true
/// Clause: 5.2.33, 6.3.18, 6.3.20, 6.23.3.1
/// Invalid type: * (the every-type selector under test, held to one entity by its ns-scoped id)
#[gtest]
#[tokio::test]
async fn n_qry_ent_9011_post_a_wildcard_selector_beside_another_refuses_local_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "wild-1")?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", "wild-2")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The client refuses `local=false` beside a `*` selector before sending, so the refused call goes out as written.
    let body = format!(
        r#"{{"type": "Query", "entities": [{{"id": "urn:ngsi-ld:Building:{namespace}-wild-1", "type": "Building_{namespace}"}}, {{"id": "urn:ngsi-ld:Vehicle:{namespace}-wild-2", "type": "*"}}]}}"#
    );
    let request = exchange(
        &broker,
        Method::POST,
        &target("entityOperations/query", &[("local", "false")]),
        &[("Content-Type", "application/json"), ("Accept", "application/json")],
        Some(body.as_str()),
    )?;
    let refused = broker.send_unchecked(request).await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors(&building_id, &building_type, &vehicle_id)?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .locality(Locality::Local)
        .selectors(selectors(&building_id, &building_type, &vehicle_id)?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    Ok(())
}

/// The selector of the Building by id and type beside the every-type selector of the Vehicle id.
fn selectors(building_id: &EntityId, building_type: &EntityTypeName, vehicle_id: &EntityId) -> Result<[EntitySelector; 2]> {
    Ok([
        EntitySelector::new(SelectorTypeBuilder::from(building_type).build()?).with_ids(building_id.clone()),
        EntitySelector::all().with_ids(vehicle_id.clone()),
    ])
}
