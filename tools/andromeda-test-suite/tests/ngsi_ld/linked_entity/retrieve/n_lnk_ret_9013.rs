//! Join level without a retrieving join is invalid request.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with `joinLevel=2` and no join, or with join=@none, answers 400 `InvalidRequest`, while
/// join=flat with `joinLevel=2` answers 200 with the linked entity
/// Clause: 6.3.20, Table 6.5.3.1-1, 4.5.23
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9013_join_level_without_a_retrieving_join_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let person_id = namespace.urn("Person", 1)?;
    let person_type = namespace.entity_type("Person")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let person = Entity::builder()
        .id(person_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .build()?;
    broker.entities().create(&person).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .relationship("owner", person_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .join_level(2.try_into()?)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_detail_containing("joinLevel")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_id}?join=%40none&joinLevel=2"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_id)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), contains(has_id(&person_id)));
    Ok(())
}
