//! An unparsable accept language is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
};

/// Pins: Retrieve Entity and Query Entities with Accept-Language `en_US`, which is not a language tag, answer 400
/// `BadRequestData` as an unparsable lang does, and the refusal varies on accept-language
/// Clause: 4.15, 5.7.1.4, 5.7.2.4
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0007_an_unparsable_accept_language_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The typed Accept-Language takes language ranges only, so the read is sent with the field as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_id}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(HeaderName::from_static("accept-language"), HeaderValue::from_static("en_US"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_header_containing(HeaderName::from_static("vary"), "accept-language")
    );
    // The typed Accept-Language takes language ranges only, so the read is sent with the field as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(HeaderName::from_static("accept-language"), HeaderValue::from_static("en_US"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
