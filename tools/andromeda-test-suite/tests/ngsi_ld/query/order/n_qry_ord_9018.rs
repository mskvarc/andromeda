//! A collation that is no language tag is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: `orderBy=name` with collation=not a collation answers 400 `BadRequestData`, and with collation=und-x-icu
/// answers 200
/// Clause: 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_9018_a_collation_that_is_no_language_tag_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?.to_string();
    // The client refuses a collation that is no language tag, so each read goes out as written.
    let fields = [("Accept", "application/json")];
    let refused = target(
        "entities",
        &[
            ("type", building_type.as_str()),
            ("local", "true"),
            ("orderBy", "name"),
            ("collation", "not a collation"),
        ],
    );
    let answer = broker.send_unchecked(exchange(&broker, Method::GET, &refused, &fields, None)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let admitted = target(
        "entities",
        &[
            ("type", building_type.as_str()),
            ("local", "true"),
            ("orderBy", "name"),
            ("collation", "und-x-icu"),
        ],
    );
    let answer_2 = broker.send_unchecked(exchange(&broker, Method::GET, &admitted, &fields, None)?).await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    Ok(())
}
