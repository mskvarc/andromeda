//! A pattern that is no regular expression is refused on one line.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities with `q=p~="(a"` or with `idPattern=[` answers 400 `BadRequestData` whose detail names the
/// parameter and the rule the pattern breaks on one line, without the parser's drawing of the pattern
/// Clause: 4.9, 5.5.4, 5.7.2.4, 6.3.20
/// Recorded: clause 6.3.20 fixes the error type but not the wording; Andromeda names the parameter and the rule the
/// pattern breaks
#[gtest]
#[tokio::test]
async fn o_cmn_val_9044_a_pattern_that_is_no_regular_expression_is_refused_on_one_line() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    for (target, detail) in [
        (
            format!("entities?type=Vehicle_{namespace}&q=p~%3D%22%28a%22"),
            "invalid query parameter q: invalid regular expression: unclosed group",
        ),
        (
            format!("entities?type=Vehicle_{namespace}&idPattern=%5B"),
            "invalid query parameter idPattern: invalid regular expression: unclosed character class",
        ),
    ] {
        // Every request carries a pattern the client would not write, so each is sent as written.
        let answer = broker.send_unchecked(request_to(&broker, Method::GET, &target)?).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail),
            "{target}"
        );
    }
    Ok(())
}
