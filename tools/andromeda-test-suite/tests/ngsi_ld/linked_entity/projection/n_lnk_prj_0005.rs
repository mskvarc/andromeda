//! A nested term without a retrieving join is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: a pick or omit term nesting into a Linked Entity is `BadRequestData` on Query Entities and on Retrieve
/// Entity when no join retrieves Linked Entities, with join absent or @none (its default), and the same nesting is
/// answered once a join retrieves the Linked Entity
/// Clause: 5.7.1.4, 5.7.2.4, 4.21, 4.5.23
#[gtest]
#[tokio::test]
async fn n_lnk_prj_0005_a_nested_term_without_a_retrieving_join_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(sensor_id.clone())
            .entity_type(sensor_type)
            .property("humidity", 40)
            .build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .relationship("sensor", sensor_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("sensor{humidity}".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&join=%40none&pick=sensor%7Bhumidity%7D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .pick("sensor{humidity}".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .omit("sensor{humidity}".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // the same nesting is answered once a join retrieves the Linked Entity
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .pick("id,sensor{humidity}".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, some(eq(&building_id)));
    expect_that!(
        linked_entities(entity, &["sensor"]),
        nth(0, has_attribute("humidity", any_attribute().value(40)))
    );
    Ok(())
}
