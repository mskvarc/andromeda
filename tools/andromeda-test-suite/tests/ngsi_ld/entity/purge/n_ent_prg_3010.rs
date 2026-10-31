//! A type selection outside the grammar is bad request data and purges nothing.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Locality, type_selection},
};

/// Pins: Purge Entities refuses a type selection outside the 4.17 grammar with `BadRequestData` and deletes
/// nothing, and purges the entities a parenthesised conjunction selects
/// Clause: 4.17, 5.6.21.4, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_3010_a_type_selection_outside_the_grammar_is_bad_request_data_and_purges_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let home_type = namespace.entity_type("Home")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let thing_2_id = namespace.urn("Thing", 2)?;
    let entities = [
        Entity::builder()
            .id(thing_id.clone())
            .entity_type(vec![home_type.clone(), vehicle_type.clone()])
            .build()?,
        Entity::builder().id(thing_2_id.clone()).entity_type(home_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 4.17: "For logical and grouping parenthesis are needed"
    // The selection is outside the grammar the model writes, so it is sent as written.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type=Home_{namespace};Vehicle_{namespace}&local=true"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let request_2 = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type=((Home_{namespace};Vehicle_{namespace}))&local=true"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&thing_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&thing_id));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&thing_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&thing_2_id));
    let purged = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(type_selection::all_of([&home_type, &vehicle_type]).build()?)
        .locality(Locality::Local)
        .send()
        .await?;
    purged.outcome.done()?;
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&thing_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&thing_2_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&thing_2_id));
    Ok(())
}
