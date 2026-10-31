//! A source paging 100 of 250 instances answers a retrieve with 206 and the resumed request with the rest.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    cut_stream::{self, instant},
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::NgsiLdMediaType,
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity over an inclusive registration whose source pages 100 of its 250
/// instances answers 206 with a `Content-Range` ending at the last instant before the source's cut, 99 instances and
/// a next link one microsecond past that end, and the request resumed there answers 200 with the remaining 151
/// instances and no `Content-Range`
/// Clause: 6.3.10, 5.7.3.4
/// Recorded: clause 6.3.10 does not say whether a source's range-end carries every instance at its instant; Andromeda
/// leaves a source's instances at that instant to the next page
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9027_a_source_paging_100_of_250_instances_answers_a_retrieve_with_206_and_the_resumed_request_with_the_rest() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = cut_stream::registration(&source, &namespace, &vehicle_id, &vehicle_type)?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration_of(&registration);
    cut_stream::serve(&source, &vehicle_id, &vehicle_type).await?;
    let first = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(first.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        first.head.headers().get(CONTENT_RANGE),
        some(eq(&HeaderValue::from_static("DateTime 2020-01-01T00:00:00Z-2020-01-01T01:38:00Z/*")))
    );
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    expect_that!(
        link.find_by_rel(&RelationType::Next).next().map(|value| value.target().as_str()),
        some(contains_substring("timeAt=2020-01-01T01%3A38%3A00.000001Z"))
    );
    expect_that!(
        first.outcome.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(99)),
                nth(0, any_attribute().observed_at(instant(0)?)),
                nth(98, any_attribute().observed_at(instant(98)?))
            )
        )
    );
    let resumed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2020-01-01T01:38:00.000001Z".parse()?))
        .send()
        .await?;
    expect_that!(resumed.status, eq(StatusCode::OK));
    expect_that!(resumed.head.headers().get(CONTENT_RANGE), none());
    expect_that!(
        resumed.outcome.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(151)),
                nth(0, any_attribute().observed_at(instant(99)?)),
                nth(150, any_attribute().observed_at(instant(249)?))
            )
        )
    );
    broker.registrations().delete(registration.id().or_fail()?).send().await?;
    Ok(())
}
