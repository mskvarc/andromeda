//! A source cut short forwards narrows the hub history.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    evolution_matcher::{has_evolution, lacks_evolution},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{NormalizedTemporal, QueryTemporalEvolution},
    },
    model::{Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{IdPattern, builders::SelectorTypeBuilder, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: Query Temporal Evolution of Entities over an inclusive registration whose source answers 206 with a
/// Content-Range ending on 2 January 2020 answers 206 with a Content-Range from the requested start to the broker's own
/// instance of 1 January, the source's instance at its range-end and the broker's own instance of 3 January left out,
/// and a request resumed one microsecond past that end answers both
/// Clause: 6.3.10, 5.7.4.4
/// Recorded: clause 6.3.10 does not say whether a source's range-end carries every instance at its instant; Andromeda
/// leaves a source's instances at that instant to the next page, which reads them again from the source, unless the
/// page would otherwise hold none
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9006_a_source_cut_short_forwards_narrows_the_hub_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let partial_type = namespace.entity_type("PartialTemporal")?;
    let remote_id = namespace.urn("PartialTemporal", "remote")?;
    let local_id = namespace.urn("PartialTemporal", "local")?;
    let registration_id = namespace.registration_id(1)?;
    let remote_evolution = TemporalEntity::builder()
        .id(remote_id.clone())
        .entity_type(partial_type.clone())
        .property("speed", Property::builder(2).observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?;
    // The source answers its one Entity's history cut short, the Content-Range saying where it stopped.
    let cut_answer = ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
        .set_body_raw(format!("[{}]", remote_evolution.render(Lossless::Normalized)), "application/json")
        .insert_header(CONTENT_RANGE, HeaderValue::from_static("DateTime 2019-01-01T00:00:00Z-2020-01-02T00:00:00Z/*"));
    let rest_answer =
        ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_raw(format!("[{}]", remote_evolution.render(Lossless::Normalized)), "application/json");
    source
        .answer_in_turn(
            || Mock::given(method("GET")).and(path("/a/ngsi-ld/v1/temporal/entities")),
            vec![cut_answer, rest_answer],
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(partial_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let local_evolution = TemporalEntity::builder()
        .id(local_id.clone())
        .entity_type(partial_type.clone())
        .property("speed", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&local_evolution).send().await?.created()?;
    let selector = SelectorTypeBuilder::from(&partial_type).build()?;
    let temporal = temporal_query::after("2019-01-01T00:00:00Z".parse()?).build()?;
    let query = QueryTemporalEvolution::builder()
        .entity_type(&selector)
        .temporal_q(&temporal)
        .format(NormalizedTemporal)
        .build();
    let first = broker.with_heads().with_accept(Accept::new(NgsiLdMediaType::Json)).send(&query).await?;
    expect_that!(first.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        first.head.headers().get(CONTENT_RANGE),
        some(eq(&HeaderValue::from_static("DateTime 2019-01-01T00:00:00Z-2020-01-01T00:00:00Z/*")))
    );
    let items = first.outcome.items();
    let local = items.iter().find(|evolution| evolution.id == local_id).or_fail()?;
    expect_that!(
        local,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().observed_at("2020-01-01T00:00:00Z".parse()?))))
    );
    expect_that!(
        items.iter().find(|evolution| evolution.id == remote_id),
        any![none(), some(any![lacks_evolution("speed"), has_evolution("speed", is_empty())])]
    );
    let second = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00.000001Z".parse()?))
        .entity_type(&partial_type)
        .send()
        .await?;
    expect_that!(second.status, eq(StatusCode::OK));
    expect_that!(second.head.headers().get(CONTENT_RANGE), none());
    expect_that!(
        second.outcome.items(),
        contains(all!(
            has_id(&local_id),
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().observed_at("2020-01-03T00:00:00Z".parse()?))))
        ))
    );
    expect_that!(
        second.outcome.items(),
        contains(all!(
            has_id(&remote_id),
            has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().observed_at("2020-01-02T00:00:00Z".parse()?))))
        ))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
