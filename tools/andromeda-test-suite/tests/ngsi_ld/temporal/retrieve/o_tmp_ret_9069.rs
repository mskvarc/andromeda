//! The next link is typed as the response.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    link_header::link_text,
    namespace::Namespace,
    outcome::Upserted,
    paged_history::series,
};
use googletest::prelude::*;
use http::{StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType, query::temporal_query};

/// Pins: The rel="next" Link of a partial Temporal Evolution read with Accept application/json is typed
/// application/json, and read with Accept application/ld+json is typed application/ld+json
/// Clause: 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// lastN and with lastN (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 defines no request that continues a partial response; Andromeda points a partial retrieve at
/// one with a rel="next" Link typed as the response, which opens one microsecond past a forward page's last instant and
/// asks a backward page's continuation for the client's lastN less the fewest instances any attribute received
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9069_the_next_link_is_typed_as_the_response() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let speed = (0..20_i32).map(|value| (value, value.unsigned_abs())).collect::<Vec<_>>();
    let paged_series_typed_evolution = series(&namespace, "typed", &speed, &[])?;
    let paged_series_typed_id = paged_series_typed_evolution.id.clone();
    broker.temporal().create_or_update(&paged_series_typed_evolution).send().await?.created()?;
    for (media_type, text) in [(NgsiLdMediaType::Json, "application/json"), (NgsiLdMediaType::JsonLd, "application/ld+json")] {
        let headed = broker
            .with_heads()
            .with_accept(Accept::new(media_type))
            .temporal()
            .retrieve(&paged_series_typed_id)
            .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
            .send()
            .await?;
        expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
        let content_type = headed.head.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok());
        expect_that!(content_type, some(contains_substring(text)));
        let links = link_text(headed.head.headers());
        expect_that!(
            &links,
            contains_substring(format!(
                "</ngsi-ld/v1/temporal/entities/{paged_series_typed_id}?timerel=after&timeAt=2024-01-01T00%3A18%3A00.000001Z>; rel=\"next\"; type=\"{text}\""
            ))
        );
        expect_that!(headed.outcome.body(), has_evolution("speed", len(eq(19))));
        expect_that!(headed.outcome.body(), has_evolution("speed", contains(any_attribute().value(0))));
    }
    Ok(())
}
