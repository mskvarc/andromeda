//! Retrieve with an empty language is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with an empty lang parameter is refused as 400 `BadRequestData`
/// Clause: 4.15, 5.7.1.5
/// Recorded: clause 4.15 defines lang as a langtag, which an empty value is not, but no clause names the error for
/// it; Table 5.5.2-1 gives `BadRequestData` to a request whose input data does not meet the requirements of the
/// operation and `InvalidRequest` to one that is syntactically invalid, and the parameter is input data of Retrieve
/// Entity
#[gtest]
#[tokio::test]
async fn o_qry_lng_0009_retrieve_with_an_empty_language_is_bad_request_data() -> Result<()> {
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
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_id}?lang="),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
