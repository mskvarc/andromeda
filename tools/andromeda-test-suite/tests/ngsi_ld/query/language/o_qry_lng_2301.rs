//! Query entities with an empty language is refused.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem_among, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
};

/// Pins: Query Entities with an empty lang parameter is refused with 400, `BadRequestData` or `InvalidRequest`
/// Clause: 4.15, 5.7.2.3
/// Recorded: clause 4.15 defines lang as a langtag, which an empty value is not, but no clause names the error for
/// it; Table 5.5.2-1 makes `BadRequestData` and `InvalidRequest` both plausible, so both are accepted
#[gtest]
#[tokio::test]
async fn o_qry_lng_2301_query_entities_with_an_empty_language_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&lang="),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    Ok(())
}
