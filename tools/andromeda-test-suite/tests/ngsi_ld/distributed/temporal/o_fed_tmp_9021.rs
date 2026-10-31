//! A source cut before the local one leaves the later local instances to the next page.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    followed_link,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    StatusCode,
    header::{ACCEPT, CONTENT_RANGE},
};
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::{DateTime, Lossless, NgsiLdMediaType, Output, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use std::str;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path_regex},
};

/// Pins: Retrieve Temporal Evolution of an Entity holding 25 local instances, one a minute from 01:00, whose inclusive
/// source answers 206 ending at 01:10, answers 206 ending at 01:09 with the local instances before 01:10 and the
/// source's before it, the local page's later instances left out, and its rel="next" Link answers a 200 with the rest,
/// every instance answered once
/// Clause: 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 does not say whether a source's range-end carries every instance at its instant; Andromeda
/// leaves a source's instances at that instant to the next page, which reads them again from the source, unless the
/// page would otherwise hold none
/// Recorded: clause 6.3.10 defines no request that continues a partial response; Andromeda points a partial retrieve at
/// one with a rel="next" Link typed as the response, which opens one microsecond past a forward page's last instant and
/// asks a backward page's continuation for the client's `lastN` less the fewest instances any attribute received
#[gtest]
#[tokio::test]
async fn o_fed_tmp_9021_a_source_cut_before_the_local_one_leaves_the_later_local_instances_to_the_next_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let partial_temporal_id = namespace.urn("PartialTemporal", 1)?;
    let partial_temporal_type = namespace.entity_type("PartialTemporal")?;
    let registration_id = namespace.registration_id(1)?;
    let cut_evolution = TemporalEntity::builder()
        .id(partial_temporal_id.clone())
        .entity_type(partial_temporal_type.clone())
        .property("speed", Property::builder(100).observed_at("2020-01-01T01:05:00Z".parse()?))
        .property("speed", Property::builder(101).observed_at("2020-01-01T01:10:00Z".parse()?))
        .build()?;
    let rest_evolution = TemporalEntity::builder()
        .id(partial_temporal_id.clone())
        .entity_type(partial_temporal_type.clone())
        .property("speed", Property::builder(101).observed_at("2020-01-01T01:10:00Z".parse()?))
        .property("speed", Property::builder(102).observed_at("2020-01-01T01:20:00Z".parse()?))
        .build()?;
    let cut_answer = ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
        .set_body_raw(cut_evolution.render(Lossless::Normalized).to_string(), "application/json")
        .insert_header(CONTENT_RANGE, HeaderValue::from_static("DateTime 2020-01-01T00:00:00Z-2020-01-01T01:10:00Z/*"));
    let rest_answer = ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_raw(rest_evolution.render(Lossless::Normalized).to_string(), "application/json");
    source
        .answer_in_turn(
            || Mock::given(method("GET")).and(path_regex("^/a/ngsi-ld/v1/temporal/entities/[^/]+$")),
            vec![cut_answer, rest_answer],
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(partial_temporal_type.clone()).with_id(partial_temporal_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let instants = (0..25)
        .map(|minute| format!("2020-01-01T01:{minute:02}:00Z").parse::<DateTime>())
        .collect::<Result<Vec<_>, _>>()?;
    let local_evolution = instants
        .iter()
        .zip(0..)
        .fold(
            TemporalEntity::builder()
                .id(partial_temporal_id.clone())
                .entity_type(partial_temporal_type.clone()),
            |builder, (instant, value): (&DateTime, i32)| builder.property("speed", Property::builder(value).observed_at(*instant)),
        )
        .build()?;
    broker.temporal().create_or_update(&local_evolution).send().await?.created()?;
    let first = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .retrieve(&partial_temporal_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(first.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        first.head.headers().get(CONTENT_RANGE),
        some(eq(&HeaderValue::from_static("DateTime 2020-01-01T00:00:00Z-2020-01-01T01:09:00Z/*")))
    );
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    expect_that!(
        link.find_by_rel(&RelationType::Next).next().map(|value| value.target().as_str()),
        some(contains_substring("timeAt=2020-01-01T01%3A09%3A00.000001Z"))
    );
    let evolution = first.outcome.body();
    expect_that!(evolution, has_evolution("speed", len(eq(11))));
    for value in (0..=9).chain([100]) {
        expect_that!(evolution, has_evolution("speed", contains(any_attribute().value(value))));
    }
    let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(request).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(answer.headers().get(CONTENT_RANGE), none());
    let continued = TemporalEntity::<Output>::parse(str::from_utf8(answer.body())?, Lossless::Normalized)?;
    expect_that!(&continued, has_evolution("speed", len(eq(17))));
    for value in (10..=24).chain([101, 102]) {
        expect_that!(&continued, has_evolution("speed", contains(any_attribute().value(value))));
    }
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
