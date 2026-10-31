//! The same context link twice is refused.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::LINK};
use ngsi_ld::model::{Entity, ErrorType};

/// Pins: Retrieve Entity carrying the same @context link twice, in two Link lines or in one, is refused as
/// `BadRequestData`
/// Clause: 6.3.5, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0045_the_same_context_link_twice_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The typed client writes one context Link, so the doubled Link the test refuses is sent as written.
    let core_link = format!(r#"<{core}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#);
    let request = request_to(&broker, Method::GET, &format!("entities/{vehicle_id}"))?
        .append_header(LINK, HeaderValue::from_str(&core_link)?)
        .append_header(LINK, HeaderValue::from_str(&core_link)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let request_2 =
        request_to(&broker, Method::GET, &format!("entities/{vehicle_id}"))?.append_header(LINK, HeaderValue::from_str(&format!("{core_link}, {core_link}"))?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
