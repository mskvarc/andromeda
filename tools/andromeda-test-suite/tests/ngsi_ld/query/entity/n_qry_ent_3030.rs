//! Pick with no narrowing input is too wide in every get combination.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: a GET query whose only input besides pick is no narrowing input of 5.7.2.4 (an id list, an `idPattern`, a
/// `scopeQ`, a lang, a page or representation parameter) is too wide, whatever the pick names, and the same pick
/// beside a type selector is answered
/// Clause: 5.7.2.4, 6.4.3.2, 4.21
#[gtest]
#[tokio::test]
async fn n_qry_ent_3030_pick_with_no_narrowing_input_is_too_wide_in_every_get_combination() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .scope("/Madrid")
        .property("p", 1)
        .property("q", 2)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 5.7.2.4: the inputs are a selector of Entity Types, a list of Attribute names, an NGSI-LD Query, a GeoQuery or the local scope; pick is a projection, none of them
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, "entities?pick=p%2Cq")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the entity identifier, without type or attributes, is not a restriction either ("it is not possible to retrieve a set of entities by only specifying desired Entity identifiers")
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3ABuilding%3A{namespace}-1&pick=p"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?idPattern=%5Eurn%3Angsi-ld%3ABuilding%3A{namespace}-.%2A&pick=p"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "entities?scopeQ=%2FMadrid&pick=p",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // representation, language and page parameters do not narrow
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "entities?pick=p&options=keyValues&limit=10",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, "entities?pick=p&lang=en")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a pick naming a system attribute names no narrowing attribute
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, "entities?pick=createdAt")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // beside a type selector the same pick is answered, and the projection applies
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("id,p".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(has_id(&building_id), has_attribute("p", any_attribute().value(1)), lacks_attribute("q"))
        )
    );
    Ok(())
}
