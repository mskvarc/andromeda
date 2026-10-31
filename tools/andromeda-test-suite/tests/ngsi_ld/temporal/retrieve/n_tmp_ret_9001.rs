//! Scope in attrs is refused and pick selects it.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, OneOrMany, Property, Scope, evolution::ScopeValue},
    query::projection::Projection,
};

/// Pins: Retrieve Temporal Evolution of an Entity with attrs=speed,scope answers 400 `BadRequestData` naming scope, with
/// pick=id,type,speed,scope answers the reified Scope, and with attrs=speed answers speed 60
/// Clause: Table 6.19.3.1-1, 4.18, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9001_scope_in_attrs_is_refused_and_pick_selects_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .scope("/Madrid/Centro")
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let refused = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .attrs(vec!["speed".parse()?, "scope".parse()?])
        .send()
        .await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("scope")
    );
    let picked = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_projected(&vehicle_id)
        .pick("id,type,speed,scope".parse::<Projection>()?)
        .send()
        .await?;
    let first = picked.body().scope.first().or_fail()?;
    expect_that!(first.value(), eq(&ScopeValue::Scopes(OneOrMany::one("/Madrid/Centro".parse::<Scope>()?))));
    let narrowed = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(narrowed.body(), has_evolution("speed", contains(any_attribute().value(60))));
    Ok(())
}
