//! A purge under a malformed entity map header is bad request data and deletes nothing.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Purge Entities with an `NGSILD-EntityMap` header that is not a URI answers 400 `BadRequestData` naming the
/// header and deletes no Entity
/// Clause: Table 6.4.3.3-2, 5.5.9.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_9011_a_purge_under_a_malformed_entity_map_header_is_bad_request_data_and_deletes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The header is not a URI, which the client cannot state, so the request is sent as written.
    let request = request_to(&broker, Method::DELETE, &format!("entities?type=Building_{namespace}"))?
        .append_header(HeaderName::from_static("ngsild-entitymap"), HeaderValue::from_static("not-a-uri"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the ngsild-entitymap header is not a uri")
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    Ok(())
}
