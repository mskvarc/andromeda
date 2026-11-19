//! A temporal query parameter value it does not take is named with what it takes.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Temporal Evolution of Entities with `lastN=0` or `lastN=-1` answers 400 `BadRequestData` whose detail
/// says `lastN` is not a positive integer, with `timerel=bogus` one naming `bogus` and the relationships `timerel`
/// takes, and with `aggrMethods=bogus` one naming `bogus` and the methods `aggrMethods` takes; no detail carries
/// "variant" or the integer parser's words
/// Clause: 4.5.19, 4.11, 5.5.4, 6.3.20, Table 6.18.3.2-1
/// Recorded: clause 6.3.20 fixes the error type but not the wording; Andromeda names the parameter, the value sent
/// where it is none of a parameter's tokens, and what the parameter takes
#[gtest]
#[tokio::test]
async fn o_cmn_val_9043_a_temporal_query_parameter_value_it_does_not_take_is_named_with_what_it_takes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let window = format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2026-03-01T00:00:00Z");
    for (target, detail) in [
        (format!("{window}&lastN=0"), "invalid query parameter lastN: not a positive integer"),
        (format!("{window}&lastN=-1"), "invalid query parameter lastN: not a positive integer"),
        (
            format!("temporal/entities?type=Vehicle_{namespace}&timerel=bogus&timeAt=2026-03-01T00:00:00Z"),
            "invalid query parameter timerel: 'bogus' is not one of before, after, between",
        ),
        (
            format!("{window}&format=aggregatedValues&aggrMethods=bogus&aggrPeriodDuration=PT1H"),
            "invalid query parameter aggrMethods: 'bogus' is not one of avg, min, max, sum, sumsq, stddev, totalCount, distinctCount",
        ),
    ] {
        // Every request carries a parameter value the client cannot write, so each is sent as written.
        let answer = broker.send_unchecked(request_to(&broker, Method::GET, &target)?).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail),
            "{target}"
        );
    }
    Ok(())
}
