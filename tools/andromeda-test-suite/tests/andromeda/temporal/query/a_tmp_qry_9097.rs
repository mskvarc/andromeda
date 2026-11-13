//! A posted temporal query's page link followed with a selection beside its digest is bad request data.

use andromeda_test_suite::{broker, followed_link::request_with, namespace::Namespace, outcome::Upserted, problem::problem};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        headers::{Accept, Link, RelationType},
        operation::QueryMethod,
    },
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: following the next Link of Query Temporal Evolution of Entities through POST
/// `/temporal/entityOperations/query` answers the next page unchanged, and with `q`, `type` or `timerel` and
/// `timeAt` added beside `postedQuery` answers 400 `BadRequestData`
/// Clause: 5.5.9.1, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9097_a_posted_temporal_query_page_link_followed_with_a_selection_beside_its_digest_is_bad_request_data() -> Result<()> {
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
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .with_heads()
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .limit(2)
        .send()
        .await?;
    let followed = json.send(&first.outcome.next_page().or_fail()?).await?;
    expect_that!(followed.items(), len(eq(1)));
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    let entity_type = vehicle_type.to_string();
    let additions: [&[(&str, &str)]; 3] = [
        &[("q", "speed%3E1")],
        &[("type", entity_type.as_str())],
        &[("timerel", "after"), ("timeAt", "2025-01-01T00%3A00%3A00Z")],
    ];
    for added in additions {
        let request = request_with(&broker, &link, &RelationType::Next, added)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
