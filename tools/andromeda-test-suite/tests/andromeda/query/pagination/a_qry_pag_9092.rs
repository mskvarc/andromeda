//! A posted query's page link followed with a selection beside its digest is bad request data.

use andromeda_test_suite::{broker, followed_link::request_with, namespace::Namespace, paged_buildings::PagedBuildings, problem::problem};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        headers::{Accept, Link, RelationType},
        operation::QueryMethod,
    },
    model::{ErrorType, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: following the next Link of Query Entities through POST `/entityOperations/query` answers the next page
/// unchanged, and with `q`, `type` or `id` added beside `postedQuery` answers 400 `BadRequestData`
/// Clause: 5.5.9.1, 6.3.10, 6.23.3.1
#[gtest]
#[tokio::test]
async fn a_qry_pag_9092_a_posted_query_page_link_followed_with_a_selection_beside_its_digest_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .with_heads()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&buildings.entity_type)
        .q(q::attr("height").gt(5))
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let followed = json.send(&first.outcome.next_page().or_fail()?).await?;
    expect_that!(followed.items(), len(eq(2)));
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    let entity_type = buildings.entity_type.to_string();
    let id = buildings.ids[0].to_string();
    for added in [("q", "height%3E25"), ("type", entity_type.as_str()), ("id", id.as_str())] {
        let request = request_with(&broker, &link, &RelationType::Next, &[added])?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
