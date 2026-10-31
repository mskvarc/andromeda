//! Scope in attrs is refused and pick selects it.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, spelling_matcher::alone, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, ErrorType, NgsiLdMediaType, Property, Scope},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with attrs=speed,scope answers 400 `BadRequestData` naming scope, and with
/// pick=id,type,speed,scope and `keyValues` answers the Scope
/// Clause: Table 6.5.3.1-1, 4.18, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9001_scope_in_attrs_is_refused_and_pick_selects_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .scope(["/Madrid/Centro"])
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // An attribute name cannot be `scope`, so the projection is sent as written.
    let request = request_to(&broker, Method::GET, &format!("entities/{vehicle_id}?attrs=speed%2Cscope"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("scope")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&vehicle_id)
        .pick("id,type,speed,scope".parse::<Projection>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(retrieved.body().scope.as_ref(), some(alone(&"/Madrid/Centro".parse::<Scope>()?)));
    Ok(())
}
