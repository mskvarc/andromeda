//! A collation without an order by changes nothing.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Upserted, temporal_page_body, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with collation=und-u-kn-true and no orderBy, and without local scope,
/// answers collation10, collation2, collation30, as the same query without the collation does
/// Clause: 4.23, Table 6.18.3.2-1
/// Recorded: without orderBy 4.23 fixes no order; Andromeda answers in identifier byte order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9022_a_collation_without_an_order_by_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_collation2_id = namespace.urn("CollationReading", "collation2")?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let collation_reading_collation10_id = namespace.urn("CollationReading", "collation10")?;
    let collation_reading_collation30_id = namespace.urn("CollationReading", "collation30")?;
    for id in [
        &collation_reading_collation2_id,
        &collation_reading_collation10_id,
        &collation_reading_collation30_id,
    ] {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(collation_reading_type.clone())
            .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The query model writes a collation only with an `orderBy`, so the query string is sent as written.
    let request = request_to(
        &client,
        Method::GET,
        &format!(
            "temporal/entities?timerel=after&timeAt=2020-01-01T00:00:00Z&type=CollationReading_{namespace}&id={collation_reading_collation2_id},{collation_reading_collation10_id},{collation_reading_collation30_id}&collation=und-u-kn-true"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = client.send_unchecked(request).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let entities = temporal_page_body::entities(&answer)?;
    expect_that!(
        entities.as_slice(),
        ids_in_order(&[
            collation_reading_collation10_id.clone(),
            collation_reading_collation2_id.clone(),
            collation_reading_collation30_id.clone()
        ])
    );
    let page = client
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&collation_reading_type)
        .ids(vec![
            collation_reading_collation2_id.clone(),
            collation_reading_collation10_id.clone(),
            collation_reading_collation30_id.clone(),
        ])
        .send()
        .await?;
    expect_that!(
        page.items(),
        ids_in_order(&[
            collation_reading_collation10_id,
            collation_reading_collation2_id,
            collation_reading_collation30_id
        ])
    );
    Ok(())
}
