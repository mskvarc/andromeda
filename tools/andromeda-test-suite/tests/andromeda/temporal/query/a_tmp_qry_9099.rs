//! A temporal page link naming no held temporal query is bad request data.

use andromeda_test_suite::{
    broker,
    followed_link::request_replacing,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        headers::{Accept, Link, RelationType},
        operation::QueryMethod,
    },
    model::{Entity, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// A digest of the shape the broker emits, 16 bytes in unpadded base64url, that no posted query digests to.
const UNHELD_DIGEST: &str = "AAAAAAAAAAAAAAAAAAAAAA";

/// Pins: the next Link of Query Temporal Evolution of Entities through POST `/temporal/entityOperations/query` with its
/// `postedQuery` replaced by a well-formed digest no query was held under answers 400 `BadRequestData`, and so does GET
/// `/temporal/entities` naming the digest of a posted Query Entities
/// Clause: 5.5.9.1, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9099_a_temporal_page_link_naming_no_held_temporal_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for number in 1..=3 {
        let evolution = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(number).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let first = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .limit(2)
        .send()
        .await?;
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    let unheld = request_replacing(&broker, &link, &RelationType::Next, "postedQuery", UNHELD_DIGEST)?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(unheld).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_type = namespace.entity_type("Building")?;
    for number in 1..=3 {
        let building = Entity::builder()
            .id(namespace.urn("Building", number)?)
            .entity_type(building_type.clone())
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let entities = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let digest = entities
        .next()
        .or_fail()?
        .query_pairs()
        .find(|(name, _)| name == "postedQuery")
        .map(|(_, value)| value.into_owned())
        .or_fail()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let crossed = request_to(&broker, Method::GET, &format!("temporal/entities?postedQuery={digest}&limit=2"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let crossed_answer = broker.send_unchecked(crossed).await;
    expect_that!(&crossed_answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
