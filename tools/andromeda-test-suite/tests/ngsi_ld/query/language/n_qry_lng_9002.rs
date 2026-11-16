//! A posted query whose lang is no language filter is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    client::{Answering, Client, Transport},
    http::{UncheckedRequest, headers::Accept},
    model::{ErrorType, NgsiLdMediaType},
};

/// A posted Query on `path` selecting `entity_type`, with `extra` members and the `lang` member `lang`. The client
/// cannot write a `lang` that is no language filter, so the body is sent as written.
fn posted_query<T: Transport, M: Answering>(client: &Client<T, M>, path: &str, entity_type: &str, extra: &str, lang: &str) -> Result<UncheckedRequest> {
    Ok(request_to(client, Method::POST, path)?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            r#"{{"type": "Query", "entities": [{{"type": "{entity_type}"}}]{extra}, "lang": "{lang}"}}"#
        ))))
}

/// Pins: Query Entities and Query Temporal Evolution of Entities by POST of a Query whose `lang` is `en_US`, `fr,,en`
/// or `fr;level=1` are refused with 400 `BadRequestData`, and the same Query whose `lang` is
/// `fr-CH,fr;q=0.9,*;q=0.5` answers 200
/// Clause: 4.15, 5.5.4, 6.23.3.1, 6.24, Table 5.2.23-1
/// Recorded: Table 5.2.23-1 types `lang` as the language filter of clause 4.15, so a value that is no such filter
/// is data that is not syntactically correct according to the API data type definitions, which clause 5.5.4
/// refuses as `BadRequestData`
#[gtest]
#[tokio::test]
async fn n_qry_lng_9002_a_posted_query_whose_lang_is_no_language_filter_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let building_type = format!("Building_{namespace}");
    let temporal_q = r#", "temporalQ": {"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}"#;
    for (path, extra) in [("entityOperations/query", ""), ("temporal/entityOperations/query", temporal_q)] {
        for lang in ["en_US", "fr,,en", "fr;level=1"] {
            let answer = json.send_unchecked(posted_query(&json, path, &building_type, extra, lang)?).await;
            expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{path} lang {lang}");
        }
        let ranked = json
            .send_unchecked(posted_query(&json, path, &building_type, extra, "fr-CH,fr;q=0.9,*;q=0.5")?)
            .await;
        expect_that!(&ranked, answered(StatusCode::OK), "{path}");
    }
    Ok(())
}
