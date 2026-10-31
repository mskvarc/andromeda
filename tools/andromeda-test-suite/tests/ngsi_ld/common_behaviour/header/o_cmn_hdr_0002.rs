//! A profile parameter without a value is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::LINK};
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: a create whose context Link carries a profile parameter with no value is refused as `BadRequestData` and
/// creates nothing
/// Clause: 6.3.5, 6.3.21, 6.3.2
/// Recorded: IETF RFC 8288 section 3 makes a link-param value a token or a quoted string, so profile= is a
/// malformed Link; the spec names no error type for a malformed Link header, and Andromeda answers `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_cmn_hdr_0002_a_profile_parameter_without_a_value_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let core_context = SuiteConfig::get()?.core_context().as_str();
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "bad-profile")
        .build()?;
    // The typed Link writes only a profile that has a value, so the header is sent as written.
    let link = format!(r#"<{core_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"; profile="#);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building).build())?
        .remove_header(&LINK)
        .append_header(LINK, HeaderValue::from_str(&link)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
