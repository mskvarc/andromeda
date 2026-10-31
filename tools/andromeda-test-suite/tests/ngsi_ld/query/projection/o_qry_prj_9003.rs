//! A query body follows its own operation on scope in attrs.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    temporal_page_body,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{
    Entity,
    ErrorType,
    OneOrMany,
    Property,
    Scope,
    evolution::{ScopeInstance, ScopeValue},
};

/// Pins: POST Query Entities with attrs `[speed, scope]` answers 400 `BadRequestData` naming scope, and POST Query
/// Temporal Evolution of Entities with the same attrs and a `temporalQ` answers the reified Scope
/// Clause: Table 5.2.23-1, 6.23.3.1, 6.24.3.1, 4.18
#[gtest]
#[tokio::test]
async fn o_qry_prj_9003_a_query_body_follows_its_own_operation_on_scope_in_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(OneOrMany::try_from(vec!["/Madrid/Centro".parse::<Scope>()?])?)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // `attrs` cannot name `scope`, which these Query documents do, so they go out as written.
    let fields = [("Content-Type", "application/json"), ("Accept", "application/json")];
    let entities_query = format!(r#"{{"type": "Query", "entities": [{{"id": "{vehicle_id}", "type": "{vehicle_type}"}}], "attrs": ["speed", "scope"]}}"#);
    let refused = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "entityOperations/query",
            &fields,
            Some(entities_query.as_str()),
        )?)
        .await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("scope")
    );
    let temporal_query = format!(
        r#"{{"type": "Query", "entities": [{{"id": "{vehicle_id}", "type": "{vehicle_type}"}}], "attrs": ["speed", "scope"], "temporalQ": {{"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}}}}"#
    );
    let answer = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "temporal/entityOperations/query",
            &fields,
            Some(temporal_query.as_str()),
        )?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let evolutions = temporal_page_body::entities(&answer?)?;
    expect_that!(
        evolutions.first().and_then(|evolution| evolution.scope.first()).map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one("/Madrid/Centro".parse::<Scope>()?))))
    );
    Ok(())
}
