//! A boolean after an ordering operator in csf is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q::Query,
};

/// Pins: Query Entities refuses a csf whose ordering operator is followed by true or false with `BadRequestData`,
/// through the GET parameter and the POST member, since csf is a query string as per clause 4.9, and answers the
/// Boolean after ==
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_3040_a_boolean_after_an_ordering_operator_in_csf_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder().id(sensor_id.clone()).entity_type(sensor_type.clone()).build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    // The parameter model refuses an ordering operator beside a boolean, so each call goes out as written.
    let fields = [("Accept", "application/json")];
    for csf in ["enabled>true", "enabled<=false"] {
        let target = format!("entities?type=Sensor_{namespace}&csf={}", csf.replace('>', "%3E").replace('<', "%3C"));
        let request = exchange(&broker, Method::GET, &target, &fields, None)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    let post_fields = [("Content-Type", "application/json"), ("Accept", "application/json")];
    for csf in ["enabled>=true", "enabled<false"] {
        let body = format!(r#"{{"type": "Query", "entities": [{{"type": "Sensor_{namespace}"}}], "csf": "{csf}"}}"#);
        let request = exchange(&broker, Method::POST, "entityOperations/query", &post_fields, Some(body.as_str()))?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .csf("enabled==true".parse::<Query>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_id)));
    Ok(())
}
