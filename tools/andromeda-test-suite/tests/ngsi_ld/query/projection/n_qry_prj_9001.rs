//! Scope in attrs is refused and pick selects it.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::target,
    simplified_matcher::has_value,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, OneOrMany, Property, Scope},
    query::projection::Projection,
};

/// Pins: Query Entities with attrs=speed,scope answers 400 `BadRequestData` naming scope, with
/// pick=id,type,speed,scope and `keyValues` answers the Scope, and with attrs=speed and `keyValues` answers speed
/// 60
/// Clause: Table 6.4.3.2-1, 4.18, 4.21, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_prj_9001_scope_in_attrs_is_refused_and_pick_selects_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let scope = "/Madrid/Centro".parse::<Scope>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(OneOrMany::try_from(vec![scope.clone()])?)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // `attrs` cannot name `scope`, which this read does, so it goes out as written.
    let type_text = vehicle_type.to_string();
    let id_text = vehicle_id.to_string();
    let read = target("entities", &[("type", type_text.as_str()), ("id", id_text.as_str()), ("attrs", "speed,scope")]);
    let answer = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("scope")
    );
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let picked = json
        .entities()
        .query_projected_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .pick("id,type,speed,scope".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(picked.items(), len(eq(1)));
    expect_that!(picked.items().first().and_then(|item| item.scope.as_ref()), some(eq(&OneOrMany::one(scope))));
    let attrs = json
        .entities()
        .query_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id])
        .attrs(vec!["speed".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(attrs.items(), len(eq(1)));
    expect_that!(attrs.items(), contains(has_value("speed", 60)));
    Ok(())
}
