//! Each listed input narrows an entity query on its own.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities narrowed by only a type, only attrs, only q, only a geo-query, only local=true or only
/// type=* answers 200
/// Clause: 5.7.2.4, 5.5.13, Table 5.2.33-1
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_ent_9003_each_listed_input_narrows_an_entity_query_on_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vehicle_id = namespace.urn("Vehicle", "narrowed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed = format!("speed_{namespace}");
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(speed.as_str(), Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let by_type = json.entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(by_type.items(), len(eq(1)));
    expect_that!(by_type.items(), contains(has_id(&vehicle_id)));
    let by_attrs = json.entities().query().attrs(vec![speed.parse::<AttributeName>()?]).send().await?;
    expect_that!(by_attrs.items(), len(eq(1)));
    expect_that!(by_attrs.items(), contains(has_id(&vehicle_id)));
    let by_q = json.entities().query().q(q::attr(speed.as_str()).gt(10)).send().await?;
    expect_that!(by_q.items(), len(eq(1)));
    expect_that!(by_q.items(), contains(has_id(&vehicle_id)));
    // The client holds no spelling for the wildcard type, so this read goes out as written.
    let id_text = vehicle_id.to_string();
    let wildcard = exchange(
        &broker,
        Method::GET,
        &target("entities", &[("type", "*"), ("id", id_text.as_str())]),
        &[("Accept", "application/json")],
        None,
    )?;
    let answer = broker.send_unchecked(wildcard).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let by_id = entities(&answer?)?;
    expect_that!(by_id, len(eq(1)));
    expect_that!(by_id, contains(has_id(&vehicle_id)));
    // The client refuses a query whose only selecting parameter is a geo-query or `local`, so these two go out as written.
    let open = [
        vec![("georel", "near;maxDistance==100"), ("geometry", "Point"), ("coordinates", "[1,2]")],
        vec![("local", "true")],
    ];
    for parameters in &open {
        let request = exchange(&broker, Method::GET, &target("entities", parameters), &[("Accept", "application/json")], None)?;
        expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::OK));
    }
    Ok(())
}
