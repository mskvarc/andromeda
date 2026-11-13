//! A page link naming no held query is bad request data.

use andromeda_test_suite::{broker, followed_link::request_replacing, namespace::Namespace, paged_buildings::PagedBuildings, problem::problem};
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

/// A digest of the shape the broker emits, 16 bytes in unpadded base64url, that no posted query digests to.
const UNHELD_DIGEST: &str = "AAAAAAAAAAAAAAAAAAAAAA";

/// Pins: the next Link of Query Entities through POST `/entityOperations/query` with its `postedQuery` replaced by a
/// well-formed digest no query was held under, or by text that is no digest, answers 400 `BadRequestData`
/// Clause: 5.5.9.1, 6.3.10, 6.23.3.1
#[gtest]
#[tokio::test]
async fn a_qry_pag_9094_a_page_link_naming_no_held_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let first = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&buildings.entity_type)
        .q(q::attr("height").gt(5))
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    for digest in [UNHELD_DIGEST, "not-a-digest"] {
        let request =
            request_replacing(&broker, &link, &RelationType::Next, "postedQuery", digest)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
